import {
  BoxRenderable,
  createCliRenderer,
  InputRenderable,
  InputRenderableEvents,
  TextRenderable,
} from "@opentui/core";

/**
 * The host deliberately knows only this projection contract.  The generated
 * graph model may replace this adapter without coupling OpenTUI to transport.
 */

const emptySnapshot = () => ({
  agents: [],
  transcript: [],
  graph: [],
  kanban: [],
});

function line(value) {
  return typeof value === "string" ? value : value?.text ?? "";
}

function roster(snapshot, selected) {
  return snapshot.agents.length
    ? snapshot.agents.map((agent, index) => `${index === selected ? "›" : " "} ${agent.name ?? agent.id} ${agent.status ? `(${agent.status})` : ""}`).join("\n")
    : "No agents attached";
}

function transcript(snapshot) {
  const lines = snapshot.transcript.map(line).filter(Boolean);
  return lines.length ? lines.join("\n") : "Waiting for agent output…";
}

function workarea(snapshot, view, selected) {
  const items = view === "graph" ? snapshot.graph : snapshot.kanban;
  if (!items.length) return view === "graph" ? "No graph projection" : "No kanban projection";
  return items.map((item, index) => `${index === selected ? "›" : " "} ${line(item)}`).join("\n");
}

/**
 * Render a bridge shell from an injected runtime/model adapter.
 *
 * runtime.snapshot() returns {agents, transcript, graph, kanban}; subscribe
 * returns an unsubscribe function when the projection changes.  Optional
 * callbacks receive only UI intent, leaving North transport outside this host.
 */
export async function openOpenTuiHost(runtime, options = {}) {
  const renderer = await createCliRenderer({
    exitOnCtrlC: false,
    clearOnShutdown: true,
    ...options.renderer,
  });
  let pane = "agents";
  let view = "graph";
  let agentIndex = 0;
  let workIndex = 0;
  let disposed = false;
  let unsubscribe = () => {};

  const root = new BoxRenderable(renderer, { flexDirection: "row", width: "100%", height: "100%", gap: 1, padding: 1 });
  const agentsPane = new BoxRenderable(renderer, { flexDirection: "column", flexGrow: 1, border: true, title: "Agents" });
  const workPane = new BoxRenderable(renderer, { flexDirection: "column", flexGrow: 1, border: true, title: "Work · graph" });
  const agentsText = new TextRenderable(renderer, { flexGrow: 1, wrapMode: "word" });
  const transcriptText = new TextRenderable(renderer, { flexGrow: 3, wrapMode: "word" });
  const workText = new TextRenderable(renderer, { flexGrow: 1, wrapMode: "word" });
  const agentInput = new InputRenderable(renderer, { placeholder: "Message selected agent" });
  const workInput = new InputRenderable(renderer, { placeholder: "Compose work command" });

  agentsPane.add(agentsText);
  agentsPane.add(transcriptText);
  agentsPane.add(agentInput);
  workPane.add(workText);
  workPane.add(workInput);
  root.add(agentsPane);
  root.add(workPane);
  renderer.root.add(root);

  const snapshot = () => ({ ...emptySnapshot(), ...(runtime.snapshot?.() ?? {}) });
  const render = () => {
    if (disposed) return;
    const state = snapshot();
    agentIndex = Math.max(0, Math.min(agentIndex, Math.max(0, state.agents.length - 1)));
    const items = view === "graph" ? state.graph : state.kanban;
    workIndex = Math.max(0, Math.min(workIndex, Math.max(0, items.length - 1)));
    agentsText.content = roster(state, agentIndex);
    transcriptText.content = transcript(state);
    workText.content = workarea(state, view, workIndex);
    agentsPane.title = pane === "agents" ? "Agents · active" : "Agents";
    workPane.title = `${pane === "work" ? "Work · active · " : "Work · "}${view}`;
  };

  const submit = (scope, input, selection) => {
    const value = input.value.trim();
    if (!value) return;
    input.value = "";
    void runtime.submitInput?.({ scope, input: value, selection });
  };
  agentInput.on(InputRenderableEvents.ENTER, () => submit("agent", agentInput, snapshot().agents[agentIndex]?.id));
  workInput.on(InputRenderableEvents.ENTER, () => submit("work", workInput, { view, index: workIndex }));

  root.onKeyDown = (key) => {
    const state = snapshot();
    if (key.name === "tab") {
      key.preventDefault();
      pane = pane === "agents" ? "work" : "agents";
      (pane === "agents" ? agentInput : workInput).focus();
    } else if (key.name === "v") {
      key.preventDefault();
      view = view === "graph" ? "kanban" : "graph";
    } else if (key.name === "r") {
      key.preventDefault();
      void runtime.refresh?.();
    } else if (key.name === "up" || key.name === "down") {
      key.preventDefault();
      const delta = key.name === "up" ? -1 : 1;
      if (pane === "agents") {
        agentIndex = Math.max(0, Math.min(Math.max(0, state.agents.length - 1), agentIndex + delta));
        void runtime.selectAgent?.(state.agents[agentIndex]?.id);
      } else {
        const items = view === "graph" ? state.graph : state.kanban;
        workIndex = Math.max(0, Math.min(Math.max(0, items.length - 1), workIndex + delta));
        void runtime.selectWork?.({ view, index: workIndex, item: items[workIndex] });
      }
    }
    render();
  };

  unsubscribe = runtime.subscribe?.(render) ?? (() => {});
  render();
  agentInput.focus();
  renderer.start();

  return {
    destroy() {
      if (disposed) return;
      disposed = true;
      unsubscribe();
      renderer.destroy();
    },
  };
}
