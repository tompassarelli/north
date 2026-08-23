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
declare const agentFieldText: (...args: any[]) => any;
declare const agentRouteText: (...args: any[]) => any;
declare const agentRowText: (...args: any[]) => any;
declare const applyViewVisibility: (...args: any[]) => any;
declare const bannerBox: (...args: any[]) => any;
declare const bannerLineSegments: (...args: any[]) => any;
declare const bannerPermissions: (...args: any[]) => any;
declare const bannerRevision: (...args: any[]) => any;
declare const bannerRuleLine: (...args: any[]) => any;
declare const bootView: (...args: any[]) => any;
declare const clampPanelCursor: (...args: any[]) => any;
declare const clearPanelFilter: (...args: any[]) => any;
declare const composerHint: (...args: any[]) => any;
declare const configActivationOfJson: (...args: any[]) => any;
declare const configActivationPath: (...args: any[]) => any;
declare const configActivationPathFrom: (...args: any[]) => any;
declare const configDetailLines: (...args: any[]) => any;
declare const configEmptyNote: (...args: any[]) => any;
declare const configEntryActive: (...args: any[]) => any;
declare const configHeaderKeys: (...args: any[]) => any;
declare const configHeaderRoles: (...args: any[]) => any;
declare const configHeaderShared: (...args: any[]) => any;
declare const configPanelLegend: (...args: any[]) => any;
declare const configPanelRows: (...args: any[]) => any;
declare const configQueryField: (...args: any[]) => any;
declare const configQueryRows: (...args: any[]) => any;
declare const configReferenceText: (...args: any[]) => any;
declare const configRowContextOnly: (...args: any[]) => any;
declare const configRowMatches: (...args: any[]) => any;
declare const configRowParts: (...args: any[]) => any;
declare const configRowRole: (...args: any[]) => any;
declare const configRowSearchText: (...args: any[]) => any;
declare const configSectionRows: (...args: any[]) => any;
declare const configSectionTitle: (...args: any[]) => any;
declare const configSetInspectionText: (...args: any[]) => any;
declare const configStateText: (...args: any[]) => any;
declare const configToggleVerb: (...args: any[]) => any;
declare const configUnitActive: (...args: any[]) => any;
declare const configViewIncludes: (...args: any[]) => any;
declare const configViewRows: (...args: any[]) => any;
declare const configVisibleCount: (...args: any[]) => any;
declare const detailHeight: (...args: any[]) => any;
declare const escapeRung: (...args: any[]) => any;
declare const filterCharacter: (...args: any[]) => any;
declare const filterKeyAction: (...args: any[]) => any;
declare const helpQueryRows: (...args: any[]) => any;
declare const installKeys: (...args: any[]) => any;
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
declare const setPanelQuery: (...args: any[]) => any;
declare const submitInput: (...args: any[]) => any;
declare const tabSwapView: (...args: any[]) => any;
declare const threadViewCommand: (...args: any[]) => any;
declare const threadsView: (...args: any[]) => any;
declare const transcriptBanner: (...args: any[]) => any;
declare const transcriptPlaceholder: (...args: any[]) => any;
declare const viewList: (...args: any[]) => any;
declare const viewTabIdAt: (...args: any[]) => any;

export {
  activeFocus as "active-focus",
  agentCellText as "agent-cell-text!",
  agentFieldText as "agent-field-text",
  agentRouteText as "agent-route-text!",
  agentRowText as "agent-row-text!",
  applyViewVisibility as "apply-view-visibility!",
  bannerBox as "banner-box!",
  bannerLineSegments as "banner-line-segments",
  bannerPermissions as "banner-permissions",
  bannerRevision as "banner-revision",
  bannerRuleLine as "banner-rule-line?",
  bootView as "boot-view",
  clampPanelCursor as "clamp-panel-cursor!",
  cleanupSuspend as "cleanup-suspend!",
  clearPanelFilter as "clear-panel-filter!",
  composerHint as "composer-hint",
  configActivationOfJson as "config-activation-of-json",
  configActivationPath as "config-activation-path",
  configActivationPathFrom as "config-activation-path-from",
  configDetailLines as "config-detail-lines!",
  configEmptyNote as "config-empty-note",
  configEntryActive as "config-entry-active?",
  configHeaderKeys as "config-header-keys",
  configHeaderRoles as "config-header-roles",
  configHeaderShared as "config-header-shared!",
  configPanelLegend as "config-panel-legend",
  configPanelRows as "config-panel-rows",
  configQueryField as "config-query-field",
  configQueryRows as "config-query-rows",
  configReferenceText as "config-reference-text",
  configRowContextOnly as "config-row-context-only?",
  configRowMatches as "config-row-matches?",
  configRowParts as "config-row-parts",
  configRowRole as "config-row-role",
  configRowSearchText as "config-row-search-text",
  configSectionRows as "config-section-rows",
  configSectionTitle as "config-section-title",
  configSetInspectionText as "config-set-inspection-text!",
  configStateText as "config-state-text",
  configToggleVerb as "config-toggle-verb",
  configUnitActive as "config-unit-active?",
  configViewIncludes as "config-view-includes?",
  configViewRows as "config-view-rows",
  configVisibleCount as "config-visible-count",
  detailHeight as "detail-height!",
  escapeRung as "escape-rung",
  filterCharacter as "filter-character",
  filterKeyAction as "filter-key-action",
  handleLocalCommand as "handle-local-command!",
  helpQueryRows as "help-query-rows",
  installKeys as "install-keys!",
  launchRouteFlags as "launch-route-flags",
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
  sessionBanner as "session-banner!",
  sessionBannerLines as "session-banner-lines",
  sessionBannerRuns as "session-banner-runs",
  setLaunchRoute as "set-launch-route!",
  setPanelQuery as "set-panel-query!",
  submitInput as "submit-input!",
  suspendRuntime as "suspend-runtime!",
  tabSwapView as "tab-swap-view",
  takeLaunchRouteFlags as "take-launch-route-flags!",
  threadViewCommand as "thread-view-command?",
  threadsView as "threads-view?",
  transcriptBanner as "transcript-banner?",
  transcriptPlaceholder as "transcript-placeholder",
  viewList as "view-list",
  viewTabIdAt as "view-tab-id-at!",
};
