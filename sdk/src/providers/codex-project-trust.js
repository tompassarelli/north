import { str as $$bc$str } from '../bridge/generated/beagle/core.js';

function displayed_project_root(project_root, user_home) {
  return (((project_root === user_home)) ? "~" : (((_truthy) => _truthy !== false && _truthy != null)(project_root.startsWith($$bc$str(user_home, "/")))) ? $$bc$str("~", project_root.slice(user_home.length)) : project_root);
}

function legacy_disabled_reason(project_root, codex_home) {
  return $$bc$str(project_root, " is marked as untrusted in ", codex_home, "/config.toml. ", "To load project-local config, hooks, and exec policies, mark it trusted.");
}

function current_disabled_reason(trust_key) {
  return $$bc$str(trust_key, " is marked as untrusted in the effective configuration. ", "To load project-local config, hooks, and exec policies, update its trust setting. ", "If that setting is managed by your organization, contact your administrator.");
}

function projectDisabledReasonMatches(reason, project_root, codex_home, user_home) {
  const displayed = displayed_project_root(project_root, user_home);
  return ((reason === legacy_disabled_reason(project_root, codex_home)) || ((reason === current_disabled_reason(project_root)) || (reason === current_disabled_reason(displayed))));
}

function projectConfigWarningMatches(text, project_root, codex_home, user_home) {
  const displayed = displayed_project_root(project_root, user_home);
  const legacy_identifiers = ((_logical) => (_logical !== false && _logical != null ? text.includes($$bc$str(codex_home, "/config.toml")) : _logical))(text.includes($$bc$str(project_root, "/.codex")));
  const current_full_identifiers = ((_logical) => (_logical !== false && _logical != null ? text.includes(current_disabled_reason(project_root)) : _logical))(text.includes($$bc$str(project_root, "/.codex")));
  const current_displayed_identifiers = ((_logical) => (_logical !== false && _logical != null ? text.includes(current_disabled_reason(displayed)) : _logical))(text.includes($$bc$str(displayed, "/.codex")));
  return (legacy_identifiers || (current_full_identifiers || current_displayed_identifiers));
}

export { projectConfigWarningMatches as "projectConfigWarningMatches" };
export { projectDisabledReasonMatches as "projectDisabledReasonMatches" };
