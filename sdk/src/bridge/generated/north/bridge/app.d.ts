declare function runNorthbridgeApp(options: {
  viewId?: string;
  sourceIdentity?: string;
}): Promise<unknown>;

declare function handleLocalCommand(
  runtime: unknown,
  ui: unknown,
  input: string,
): boolean;

declare function paletteOptions(frame: string, query: string): Array<{ name: string }>;

declare function parseBridgeStream(
  runtime: unknown,
  streamState: unknown,
  chunk: string,
): unknown;

declare function launchRouteFlags(
  provider: unknown,
  tier: unknown,
  model: unknown,
  effort: unknown,
): string[];

declare function setLaunchRoute(
  runtime: Record<string, unknown>,
  name: string,
  value: string,
): unknown;

declare function takeLaunchRouteFlags(
  runtime: Record<string, unknown>,
): string[];

declare function projectConversation(
  items: unknown[],
  executionId: string,
  aggregate: boolean,
): unknown[];

declare function suspendRuntime(
  runtime: unknown,
  platform: string,
  processApi: unknown,
): boolean;

declare function cleanupSuspend(
  runtime: unknown,
  processApi: unknown,
): boolean;

declare const activeFocus: (...args: any[]) => any;
declare const agentCellText: (...args: any[]) => any;
declare const agentRouteText: (...args: any[]) => any;
declare const agentRowText: (...args: any[]) => any;
declare const applyViewVisibility: (...args: any[]) => any;
declare const bannerBox: (...args: any[]) => any;
declare const bannerLineSegments: (...args: any[]) => any;
declare const bannerPermissions: (...args: any[]) => any;
declare const bannerRevision: (...args: any[]) => any;
declare const bannerRuleLine: (...args: any[]) => any;
declare const bootView: (...args: any[]) => any;
declare const clearPanelFilter: (...args: any[]) => any;
declare const composerHint: (...args: any[]) => any;
declare const configCliName: (...args: any[]) => any;
declare const configDetailLines: (...args: any[]) => any;
declare const configEntryActive: (...args: any[]) => any;
declare const configFoldRows: (...args: any[]) => any;
declare const configGateModules: (...args: any[]) => any;
declare const configHeaderRoles: (...args: any[]) => any;
declare const configKindTag: (...args: any[]) => any;
declare const configMembershipOfJson: (...args: any[]) => any;
declare const configModuleMembers: (...args: any[]) => any;
declare const configNodeExpanded: (...args: any[]) => any;
declare const configPanelLegend: (...args: any[]) => any;
declare const configPanelRows: (...args: any[]) => any;
declare const configQueryField: (...args: any[]) => any;
declare const configQueryRows: (...args: any[]) => any;
declare const configReferenceText: (...args: any[]) => any;
declare const configRowContextOnly: (...args: any[]) => any;
declare const configRowDepth: (...args: any[]) => any;
declare const configRowLabel: (...args: any[]) => any;
declare const configRowMatches: (...args: any[]) => any;
declare const configRowRole: (...args: any[]) => any;
declare const configRowScope: (...args: any[]) => any;
declare const configRowSearchText: (...args: any[]) => any;
declare const configSectionRows: (...args: any[]) => any;
declare const configSectionTitle: (...args: any[]) => any;
declare const configStateText: (...args: any[]) => any;
declare const configToggleVerb: (...args: any[]) => any;
declare const configUnitActive: (...args: any[]) => any;
declare const configViewFolds: (...args: any[]) => any;
declare const configViewIncludes: (...args: any[]) => any;
declare const configViewRows: (...args: any[]) => any;
declare const configVisibleCount: (...args: any[]) => any;
declare const detailHeight: (...args: any[]) => any;
declare const escapeRung: (...args: any[]) => any;
declare const filterCharacter: (...args: any[]) => any;
declare const filterKeyAction: (...args: any[]) => any;
declare const foldKeyAction: (...args: any[]) => any;
declare const helpQueryRows: (...args: any[]) => any;
declare const loadConfigMemberships: (...args: any[]) => any;
declare const normalizeAgents: (...args: any[]) => any;
declare const paletteEnterAction: (...args: any[]) => any;
declare const quitCommand: (...args: any[]) => any;
declare const reconcileAgentSelection: (...args: any[]) => any;
declare const refresh: (...args: any[]) => any;
declare const renderConfigPanel: (...args: any[]) => any;
declare const renderConversation: (...args: any[]) => any;
declare const renderDetailPanel: (...args: any[]) => any;
declare const renderViewTabs: (...args: any[]) => any;
declare const restoreSubmittedText: (...args: any[]) => any;
declare const rosterRowSuppressed: (...args: any[]) => any;
declare const rosterText: (...args: any[]) => any;
declare const rosterVisibleRows: (...args: any[]) => any;
declare const selectedAgentId: (...args: any[]) => any;
declare const sessionBanner: (...args: any[]) => any;
declare const sessionBannerLines: (...args: any[]) => any;
declare const sessionBannerRuns: (...args: any[]) => any;
declare const setNodeExpanded: (...args: any[]) => any;
declare const setPanelQuery: (...args: any[]) => any;
declare const submitInput: (...args: any[]) => any;
declare const tabAction: (...args: any[]) => any;
declare const tabFoldStep: (...args: any[]) => any;
declare const tabSwapView: (...args: any[]) => any;
declare const threadViewCommand: (...args: any[]) => any;
declare const transcriptBanner: (...args: any[]) => any;
declare const transcriptPlaceholder: (...args: any[]) => any;
declare const viewList: (...args: any[]) => any;
declare const viewTabIdAt: (...args: any[]) => any;

export {
  activeFocus as "active-focus",
  agentCellText as "agent-cell-text!",
  agentRouteText as "agent-route-text!",
  agentRowText as "agent-row-text!",
  applyViewVisibility as "apply-view-visibility!",
  bannerBox as "banner-box!",
  bannerLineSegments as "banner-line-segments",
  bannerPermissions as "banner-permissions",
  bannerRevision as "banner-revision",
  bannerRuleLine as "banner-rule-line?",
  bootView as "boot-view",
  cleanupSuspend as "cleanup-suspend!",
  clearPanelFilter as "clear-panel-filter!",
  composerHint as "composer-hint",
  configCliName as "config-cli-name",
  configDetailLines as "config-detail-lines!",
  configEntryActive as "config-entry-active?",
  configFoldRows as "config-fold-rows",
  configGateModules as "config-gate-modules",
  configHeaderRoles as "config-header-roles",
  configKindTag as "config-kind-tag",
  configMembershipOfJson as "config-membership-of-json",
  configModuleMembers as "config-module-members",
  configNodeExpanded as "config-node-expanded?",
  configPanelLegend as "config-panel-legend",
  configPanelRows as "config-panel-rows",
  configQueryField as "config-query-field",
  configQueryRows as "config-query-rows",
  configReferenceText as "config-reference-text",
  configRowContextOnly as "config-row-context-only?",
  configRowDepth as "config-row-depth",
  configRowLabel as "config-row-label",
  configRowMatches as "config-row-matches?",
  configRowRole as "config-row-role",
  configRowScope as "config-row-scope",
  configRowSearchText as "config-row-search-text",
  configSectionRows as "config-section-rows",
  configSectionTitle as "config-section-title",
  configStateText as "config-state-text",
  configToggleVerb as "config-toggle-verb",
  configUnitActive as "config-unit-active?",
  configViewFolds as "config-view-folds?",
  configViewIncludes as "config-view-includes?",
  configViewRows as "config-view-rows",
  configVisibleCount as "config-visible-count",
  detailHeight as "detail-height!",
  escapeRung as "escape-rung",
  filterCharacter as "filter-character",
  filterKeyAction as "filter-key-action",
  foldKeyAction as "fold-key-action",
  handleLocalCommand as "handle-local-command!",
  helpQueryRows as "help-query-rows",
  launchRouteFlags as "launch-route-flags",
  loadConfigMemberships as "load-config-memberships!",
  normalizeAgents as "normalize-agents",
  paletteEnterAction as "palette-enter-action",
  paletteOptions as "palette-options",
  parseBridgeStream as "parse-bridge-stream!",
  projectConversation as "project-conversation",
  quitCommand as "quit-command?",
  reconcileAgentSelection as "reconcile-agent-selection!",
  refresh as "refresh!",
  renderConfigPanel as "render-config-panel!",
  renderConversation as "render-conversation!",
  renderDetailPanel as "render-detail-panel!",
  renderViewTabs as "render-view-tabs!",
  restoreSubmittedText as "restore-submitted-text!",
  rosterRowSuppressed as "roster-row-suppressed?",
  rosterText as "roster-text!",
  rosterVisibleRows as "roster-visible-rows",
  runNorthbridgeApp as "run-northbridge-app!",
  selectedAgentId as "selected-agent-id",
  setLaunchRoute as "set-launch-route!",
  sessionBanner as "session-banner!",
  sessionBannerLines as "session-banner-lines",
  sessionBannerRuns as "session-banner-runs",
  setNodeExpanded as "set-node-expanded!",
  setPanelQuery as "set-panel-query!",
  suspendRuntime as "suspend-runtime!",
  submitInput as "submit-input!",
  tabAction as "tab-action",
  tabFoldStep as "tab-fold-step!",
  tabSwapView as "tab-swap-view",
  takeLaunchRouteFlags as "take-launch-route-flags!",
  threadViewCommand as "thread-view-command?",
  transcriptBanner as "transcript-banner?",
  transcriptPlaceholder as "transcript-placeholder",
  viewList as "view-list",
  viewTabIdAt as "view-tab-id-at!",
};
