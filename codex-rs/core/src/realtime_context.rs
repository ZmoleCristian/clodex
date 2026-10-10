use crate::compact::content_items_to_text;
use crate::event_mapping::is_contextual_user_message_content;
use crate::session::session::Session;
use chrono::Utc;
use codex_exec_server::LOCAL_FS;
use codex_git_utils::resolve_root_git_project_for_trust;
use codex_protocol::models::ResponseItem;
use codex_protocol::models::plaintext_agent_message_content;
use codex_protocol::protocol::AgentStatus;
use codex_protocol::protocol::SessionSource;
use codex_thread_store::ListThreadsParams;
use codex_thread_store::SortDirection;
use codex_thread_store::StoredThread;
use codex_thread_store::ThreadSortKey;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_output_truncation::TruncationPolicy;
use codex_utils_output_truncation::truncate_text;
use dirs::home_dir;
use std::cmp::Reverse;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::DirEntry;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use tracing::debug;
use tracing::info;
use tracing::warn;

const STARTUP_CONTEXT_HEADER: &str = "Startup context from Codex.\nThis is background context about recent work and machine/workspace layout. It may be incomplete or stale. Use it to inform responses, and do not repeat it back unless relevant.";
const STARTUP_CONTEXT_OPEN_TAG: &str = "<startup_context>";
const STARTUP_CONTEXT_CLOSE_TAG: &str = "</startup_context>";
const CURRENT_THREAD_SECTION_TOKEN_BUDGET: usize = 1_200;
const RECENT_WORK_SECTION_TOKEN_BUDGET: usize = 2_200;
const WORKSPACE_SECTION_TOKEN_BUDGET: usize = 1_600;
const NOTES_SECTION_TOKEN_BUDGET: usize = 300;
pub(crate) const REALTIME_TURN_TOKEN_BUDGET: usize = 300;
const MAX_RECENT_USER_REQUESTS: usize = 3;
const USER_REQUEST_TOKEN_BUDGET: usize = 120;
const MAX_CHANGED_FILES: usize = 20;
const MAX_OPEN_SUBAGENTS: usize = 10;
const MAX_RECENT_THREADS: usize = 40;
const MAX_RECENT_WORK_GROUPS: usize = 8;
const MAX_CURRENT_CWD_ASKS: usize = 8;
const MAX_OTHER_CWD_ASKS: usize = 5;
const MAX_ASK_CHARS: usize = 240;
const TREE_MAX_DEPTH: usize = 2;
const DIR_ENTRY_LIMIT: usize = 20;
const APPROX_BYTES_PER_TOKEN: usize = 4;
const NOISY_DIR_NAMES: &[&str] = &[
    ".git",
    ".next",
    ".pytest_cache",
    ".ruff_cache",
    "__pycache__",
    "build",
    "dist",
    "node_modules",
    "out",
    "target",
];

pub(crate) async fn build_realtime_startup_context(
    sess: &Session,
    budget_tokens: usize,
) -> Option<String> {
    let config = sess.get_config().await;
    let cwd = config.cwd.clone();
    let current_thread_section = {
        let history = sess.clone_history().await;
        let open_subagents = load_open_subagents(sess).await;
        build_current_thread_section(history.raw_items(), &open_subagents)
    };
    let recent_threads = load_recent_threads(sess).await;
    let recent_work_section = build_recent_work_section(&cwd, &recent_threads).await;
    let workspace_section = build_workspace_section_with_user_root(&cwd, home_dir()).await;

    if current_thread_section.is_none()
        && recent_work_section.is_none()
        && workspace_section.is_none()
    {
        debug!("realtime startup context unavailable; skipping injection");
        return None;
    }

    let mut parts = vec![STARTUP_CONTEXT_HEADER.to_string()];

    let has_current_thread_section = current_thread_section.is_some();
    let has_recent_work_section = recent_work_section.is_some();
    let has_workspace_section = workspace_section.is_some();

    if let Some(section) = format_section(
        "Current Thread",
        current_thread_section,
        CURRENT_THREAD_SECTION_TOKEN_BUDGET,
    ) {
        parts.push(section);
    }
    if let Some(section) = format_section(
        "Recent Work",
        recent_work_section,
        RECENT_WORK_SECTION_TOKEN_BUDGET,
    ) {
        parts.push(section);
    }
    if let Some(section) = format_section(
        "Machine / Workspace Map",
        workspace_section,
        WORKSPACE_SECTION_TOKEN_BUDGET,
    ) {
        parts.push(section);
    }
    if let Some(section) = format_section(
        "Notes",
        Some("Built at realtime startup from the current thread history, local thread metadata, and a bounded local workspace scan. This excludes repo memory instructions, AGENTS files, project-doc prompt blends, and memory summaries.".to_string()),
        NOTES_SECTION_TOKEN_BUDGET,
    ) {
        parts.push(section);
    }

    let context = format_startup_context_blob(&parts.join("\n\n"));
    debug!(
        approx_tokens = approx_token_count(&context),
        requested_budget_tokens = budget_tokens,
        bytes = context.len(),
        has_current_thread_section,
        has_recent_work_section,
        has_workspace_section,
        "built realtime startup context"
    );
    info!("realtime startup context: {context}");
    Some(context)
}

async fn load_recent_threads(sess: &Session) -> Vec<StoredThread> {
    match sess
        .services
        .thread_store
        .list_threads(ListThreadsParams {
            page_size: MAX_RECENT_THREADS,
            cursor: None,
            sort_key: ThreadSortKey::UpdatedAt,
            sort_direction: SortDirection::Desc,
            allowed_sources: Vec::new(),
            model_providers: None,
            cwd_filters: None,
            relation_filter: None,
            archived: false,
            section: None,
            project_id: None,
            search_term: None,
            use_state_db_only: false,
        })
        .await
    {
        Ok(page) => page.items,
        Err(err) => {
            warn!("failed to load realtime startup threads from thread store: {err}");
            Vec::new()
        }
    }
}

async fn build_recent_work_section(
    cwd: &AbsolutePathBuf,
    recent_threads: &[StoredThread],
) -> Option<String> {
    let mut groups: HashMap<PathBuf, Vec<&StoredThread>> = HashMap::new();
    for entry in recent_threads {
        let group = match AbsolutePathBuf::from_absolute_path(entry.cwd.as_path()) {
            Ok(entry_cwd) => resolve_root_git_project_for_trust(LOCAL_FS.as_ref(), &entry_cwd)
                .await
                .map(AbsolutePathBuf::into_path_buf)
                .unwrap_or_else(|| entry.cwd.clone()),
            Err(_) => entry.cwd.clone(),
        };
        groups.entry(group).or_default().push(entry);
    }

    let current_group = resolve_root_git_project_for_trust(LOCAL_FS.as_ref(), cwd)
        .await
        .map(AbsolutePathBuf::into_path_buf)
        .unwrap_or_else(|| cwd.clone().into_path_buf());
    let mut groups = groups.into_iter().collect::<Vec<_>>();
    groups.sort_by(|(left_group, left_entries), (right_group, right_entries)| {
        let left_latest = left_entries
            .iter()
            .map(|entry| entry.updated_at)
            .max()
            .unwrap_or_else(Utc::now);
        let right_latest = right_entries
            .iter()
            .map(|entry| entry.updated_at)
            .max()
            .unwrap_or_else(Utc::now);
        (
            *left_group != current_group,
            Reverse(left_latest),
            left_group.as_os_str(),
        )
            .cmp(&(
                *right_group != current_group,
                Reverse(right_latest),
                right_group.as_os_str(),
            ))
    });

    let mut sections = Vec::new();
    for (group, mut entries) in groups.into_iter().take(MAX_RECENT_WORK_GROUPS) {
        entries.sort_by_key(|entry| Reverse(entry.updated_at));
        if let Some(section) = format_thread_group(&current_group, &group, entries).await {
            sections.push(section);
        }
    }
    (!sections.is_empty()).then(|| sections.join("\n\n"))
}

fn build_current_thread_section<'a>(
    items: impl IntoIterator<Item = &'a ResponseItem>,
    open_subagents: &[String],
) -> Option<String> {
    let mut user_requests: Vec<String> = Vec::new();
    let mut last_assistant: Option<String> = None;
    let mut last_tagged: Option<String> = None;
    let mut changed_files: Vec<String> = Vec::new();

    for item in items {
        match item {
            ResponseItem::Message { role, content, .. } if role == "user" => {
                if is_contextual_user_message_content(content) {
                    continue;
                }
                let Some(text) = content_items_to_text(content)
                    .map(|text| text.trim().to_string())
                    .filter(|text| !text.is_empty())
                else {
                    continue;
                };
                user_requests.push(text);
                changed_files.clear();
            }
            ResponseItem::Message { role, content, .. } if role == "assistant" => {
                if user_requests.is_empty() {
                    continue;
                }
                let Some(text) = content_items_to_text(content)
                    .map(|text| text.trim().to_string())
                    .filter(|text| !text.is_empty())
                else {
                    continue;
                };
                if is_tagged_backend_message(&text) {
                    last_tagged = Some(text.clone());
                }
                last_assistant = Some(text);
            }
            ResponseItem::AgentMessage {
                author, content, ..
            } => {
                if user_requests.is_empty() {
                    continue;
                }
                let Some(text) = plaintext_agent_message_content(content) else {
                    continue;
                };
                last_assistant = Some(format!("Agent message from {author}:\n{text}"));
            }
            ResponseItem::FunctionCall {
                name, arguments, ..
            } if name == "apply_patch" => {
                let patch = serde_json::from_str::<serde_json::Value>(arguments)
                    .ok()
                    .and_then(|value| value.get("input")?.as_str().map(str::to_owned))
                    .unwrap_or_else(|| arguments.clone());
                collect_patch_files(&patch, &mut changed_files);
            }
            ResponseItem::CustomToolCall { name, input, .. } if name == "apply_patch" => {
                collect_patch_files(input, &mut changed_files);
            }
            _ => {}
        }
    }

    if user_requests.is_empty() {
        return None;
    }

    let mut lines = vec![
        "Continuity for this exact thread. The backend agent already has the full history; do not ask the user to repeat it.".to_string(),
        String::new(),
        "Last user requests (oldest first):".to_string(),
    ];
    let skip = user_requests.len().saturating_sub(MAX_RECENT_USER_REQUESTS);
    for (index, request) in user_requests.iter().skip(skip).enumerate() {
        let request = truncate_realtime_text_to_token_budget(request, USER_REQUEST_TOKEN_BUDGET);
        lines.push(format!("{}. {request}", index + 1));
    }

    if let Some(message) = last_tagged.or(last_assistant) {
        lines.push(String::new());
        lines.push("Last assistant message:".to_string());
        lines.push(truncate_realtime_text_to_token_budget(
            &message,
            REALTIME_TURN_TOKEN_BUDGET,
        ));
    }

    if !changed_files.is_empty() {
        lines.push(String::new());
        lines.push("Files changed in the last turn:".to_string());
        for file in changed_files.iter().take(MAX_CHANGED_FILES) {
            lines.push(format!("- {file}"));
        }
        if changed_files.len() > MAX_CHANGED_FILES {
            lines.push(format!(
                "- ... {} more",
                changed_files.len() - MAX_CHANGED_FILES
            ));
        }
    }

    lines.push(String::new());
    if open_subagents.is_empty() {
        lines.push("Open subagents: none".to_string());
    } else {
        lines.push("Open subagents:".to_string());
        for agent in open_subagents.iter().take(MAX_OPEN_SUBAGENTS) {
            lines.push(format!("- {agent}"));
        }
    }

    Some(lines.join("\n"))
}

fn is_tagged_backend_message(text: &str) -> bool {
    let text = text.trim_start();
    text.starts_with("[COMPLETE]") || text.starts_with("[STATUS]")
}

fn collect_patch_files(patch: &str, files: &mut Vec<String>) {
    for line in patch.lines() {
        let path = ["*** Update File: ", "*** Add File: ", "*** Delete File: "]
            .iter()
            .find_map(|prefix| line.strip_prefix(prefix))
            .map(str::trim);
        if let Some(path) = path
            && !path.is_empty()
            && !files.iter().any(|known| known == path)
        {
            files.push(path.to_string());
        }
    }
}

async fn load_open_subagents(sess: &Session) -> Vec<String> {
    let thread_id = sess.thread_id();
    let agents = match sess
        .services
        .agent_control
        .list(thread_id, None, &SessionSource::default(), None)
        .await
    {
        Ok(agents) => agents,
        Err(err) => {
            warn!("failed to list subagents for realtime startup context: {err}");
            return Vec::new();
        }
    };
    agents
        .into_iter()
        .filter(|agent| agent.thread_id != thread_id)
        .filter_map(|agent| {
            let status = match agent.status {
                AgentStatus::PendingInit => "starting",
                AgentStatus::Running => "running",
                AgentStatus::Interrupted => "interrupted",
                AgentStatus::Completed(_) => "completed",
                AgentStatus::Errored(_) => "errored",
                AgentStatus::Shutdown | AgentStatus::NotFound => return None,
            };
            let name = agent
                .metadata
                .agent_nickname
                .clone()
                .or_else(|| agent.metadata.agent_path.as_ref().map(ToString::to_string))
                .unwrap_or_else(|| agent.thread_id.to_string());
            Some(format!("{name}: {status}"))
        })
        .collect()
}

pub(crate) fn truncate_realtime_text_to_token_budget(text: &str, budget_tokens: usize) -> String {
    let mut truncation_budget = budget_tokens;
    loop {
        let candidate = truncate_text(text, TruncationPolicy::Tokens(truncation_budget));
        let candidate_tokens = approx_token_count(&candidate);
        if candidate_tokens <= budget_tokens {
            break candidate;
        }

        // The shared truncator adds its marker after choosing preserved
        // content, so tighten the content budget until the rendered turn
        // itself fits the per-turn cap.
        let excess_tokens = candidate_tokens.saturating_sub(budget_tokens);
        let next_budget = truncation_budget.saturating_sub(excess_tokens.max(1));
        if next_budget == 0 {
            let candidate = truncate_text(text, TruncationPolicy::Tokens(0));
            if approx_token_count(&candidate) <= budget_tokens {
                break candidate;
            }
            break String::new();
        }
        truncation_budget = next_budget;
    }
}

async fn build_workspace_section_with_user_root(
    cwd: &AbsolutePathBuf,
    user_root: Option<PathBuf>,
) -> Option<String> {
    let cwd_path = cwd.as_path();
    let git_root = resolve_root_git_project_for_trust(LOCAL_FS.as_ref(), cwd).await;
    let cwd_tree = render_tree(cwd_path);
    let git_root_tree = git_root
        .as_ref()
        .filter(|git_root| git_root.as_path() != cwd_path)
        .and_then(|git_root| render_tree(git_root.as_path()));
    let user_root_tree = user_root
        .as_ref()
        .filter(|user_root| user_root.as_path() != cwd_path)
        .filter(|user_root| {
            git_root
                .as_ref()
                .is_none_or(|git_root| git_root.as_path() != user_root.as_path())
        })
        .and_then(|user_root| render_tree(user_root));

    if cwd_tree.is_none() && git_root.is_none() && user_root_tree.is_none() {
        return None;
    }

    let mut lines = vec![
        format!("Current working directory: {}", cwd_path.display()),
        format!("Working directory name: {}", file_name_string(cwd_path)),
    ];

    if let Some(git_root) = &git_root {
        lines.push(format!("Git root: {}", git_root.display()));
        lines.push(format!("Git project: {}", file_name_string(git_root)));
    }
    if let Some(user_root) = &user_root {
        lines.push(format!("User root: {}", user_root.display()));
    }

    if let Some(tree) = cwd_tree {
        lines.push(String::new());
        lines.push("Working directory tree:".to_string());
        lines.extend(tree);
    }

    if let Some(tree) = git_root_tree {
        lines.push(String::new());
        lines.push("Git root tree:".to_string());
        lines.extend(tree);
    }

    if let Some(tree) = user_root_tree {
        lines.push(String::new());
        lines.push("User root tree:".to_string());
        lines.extend(tree);
    }

    Some(lines.join("\n"))
}

fn render_tree(root: &Path) -> Option<Vec<String>> {
    if !root.is_dir() {
        return None;
    }

    let mut lines = Vec::new();
    collect_tree_lines(root, /*depth*/ 0, &mut lines);
    (!lines.is_empty()).then_some(lines)
}

fn collect_tree_lines(dir: &Path, depth: usize, lines: &mut Vec<String>) {
    if depth >= TREE_MAX_DEPTH {
        return;
    }

    let entries = match read_sorted_entries(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    let total_entries = entries.len();

    for entry in entries.into_iter().take(DIR_ENTRY_LIMIT) {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let name = file_name_string(&entry.path());
        let indent = "  ".repeat(depth);
        let suffix = if file_type.is_dir() { "/" } else { "" };
        lines.push(format!("{indent}- {name}{suffix}"));
        if file_type.is_dir() {
            collect_tree_lines(&entry.path(), depth + 1, lines);
        }
    }

    if total_entries > DIR_ENTRY_LIMIT {
        lines.push(format!(
            "{}- ... {} more entries",
            "  ".repeat(depth),
            total_entries - DIR_ENTRY_LIMIT
        ));
    }
}

fn read_sorted_entries(dir: &Path) -> io::Result<Vec<DirEntry>> {
    let mut entries = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter(|entry| !is_noisy_name(&entry.file_name()))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        let left_is_dir = left
            .file_type()
            .map(|file_type| file_type.is_dir())
            .unwrap_or(false);
        let right_is_dir = right
            .file_type()
            .map(|file_type| file_type.is_dir())
            .unwrap_or(false);
        (!left_is_dir, file_name_string(&left.path()))
            .cmp(&(!right_is_dir, file_name_string(&right.path())))
    });
    Ok(entries)
}

fn is_noisy_name(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    name.starts_with('.') || NOISY_DIR_NAMES.iter().any(|noisy| *noisy == name)
}

fn format_section(title: &str, body: Option<String>, budget_tokens: usize) -> Option<String> {
    let body = body?;
    let body = body.trim();
    if body.is_empty() {
        return None;
    }

    let heading = format!("## {title}\n");
    let body_budget = budget_tokens.saturating_sub(approx_token_count(&heading));
    if body_budget == 0 {
        return None;
    }

    let body = truncate_realtime_text_to_token_budget(body, body_budget);
    if body.is_empty() {
        return None;
    }

    Some(format!("{heading}{body}"))
}

fn format_startup_context_blob(body: &str) -> String {
    format!("{STARTUP_CONTEXT_OPEN_TAG}\n{body}\n{STARTUP_CONTEXT_CLOSE_TAG}")
}

async fn format_thread_group(
    current_group: &Path,
    group: &Path,
    entries: Vec<&StoredThread>,
) -> Option<String> {
    let latest = entries.first()?;
    let group_label =
        if let Ok(latest_cwd) = AbsolutePathBuf::from_absolute_path(latest.cwd.as_path()) {
            if resolve_root_git_project_for_trust(LOCAL_FS.as_ref(), &latest_cwd)
                .await
                .is_some()
            {
                format!("### Git repo: {}", group.display())
            } else {
                format!("### Directory: {}", group.display())
            }
        } else {
            format!("### Directory: {}", group.display())
        };
    let mut lines = vec![
        group_label,
        format!("Recent sessions: {}", entries.len()),
        format!("Latest activity: {}", latest.updated_at.to_rfc3339()),
    ];

    if let Some(git_branch) = latest
        .git_info
        .as_ref()
        .and_then(|git| git.branch.as_deref())
        .filter(|git_branch| !git_branch.is_empty())
    {
        lines.push(format!("Latest branch: {git_branch}"));
    }

    lines.push(String::new());
    lines.push("User asks:".to_string());

    let mut seen = HashSet::new();
    let max_asks = if group == current_group {
        MAX_CURRENT_CWD_ASKS
    } else {
        MAX_OTHER_CWD_ASKS
    };

    for entry in entries {
        let Some(first_user_message) = entry.first_user_message.as_deref() else {
            continue;
        };
        let ask = first_user_message
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let dedupe_key = format!("{}:{ask}", entry.cwd.display());
        if ask.is_empty() || !seen.insert(dedupe_key) {
            continue;
        }
        let ask = if ask.chars().count() > MAX_ASK_CHARS {
            format!(
                "{}...",
                ask.chars()
                    .take(MAX_ASK_CHARS.saturating_sub(3))
                    .collect::<String>()
            )
        } else {
            ask
        };
        lines.push(format!("- {}: {ask}", entry.cwd.display()));
        if seen.len() == max_asks {
            break;
        }
    }

    (lines.len() > 5).then(|| lines.join("\n"))
}

fn file_name_string(path: &Path) -> String {
    path.file_name()
        .and_then(OsStr::to_str)
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string())
}

fn approx_token_count(text: &str) -> usize {
    text.len().div_ceil(APPROX_BYTES_PER_TOKEN)
}

#[cfg(test)]
#[path = "realtime_context_tests.rs"]
mod tests;
