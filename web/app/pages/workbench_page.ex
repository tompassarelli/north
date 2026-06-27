defmodule LodestarWeb.WorkbenchPage do
  use Hologram.Page
  use Hologram.JS

  route "/"
  layout LodestarWeb.MainLayout

  def init(_params, component, _server) do
    roster = Lodestar.Presence.roster()
    selected = roster |> List.first() |> then(&(&1 && &1.uuid))

    component
    |> put_state(
      agents: roster,
      fleet: Lodestar.Presence.fleet_tokens(roster),
      selected: selected,
      messages: if(selected, do: Lodestar.Stream.messages(selected), else: [])
    )
    |> put_action(:mount_graph)
  end

  # Cytoscape mount (after hydration).
  def action(:mount_graph, _params, component) do
    JS.exec("window.mountDag && window.mountDag('cy')")
    component
  end

  # ── agent selection ──
  def action(:select_agent, params, component), do: select(component, params.uuid)

  def action(:cycle_agent, _params, component),
    do: select(component, step(component, +1))

  def action(:nav_down, _params, component),
    do: select(component, step(component, +1))

  def action(:nav_up, _params, component),
    do: select(component, step(component, -1))

  # load the selected agent's stream server-side, push it back to state
  def command(:load_stream, params, server),
    do: put_action(server, :apply_stream, messages: Lodestar.Stream.messages(params.agent))

  def action(:apply_stream, params, component),
    do: put_state(component, :messages, params.messages)

  defp select(component, nil), do: component

  defp select(component, uuid) do
    component
    |> put_state(:selected, uuid)
    |> put_command(:load_stream, agent: uuid)
  end

  # move selection by delta within the roster (wraps)
  defp step(component, delta) do
    ids = Enum.map(component.state.agents, & &1.uuid)

    case ids do
      [] ->
        nil

      _ ->
        i = Enum.find_index(ids, &(&1 == component.state.selected)) || 0
        Enum.at(ids, Integer.mod(i + delta, length(ids)))
    end
  end

  def template do
    ~HOLO"""
    <window $key_down.shift+tab.prevent_default="cycle_agent" />
    <window $key_down.arrow_down="nav_down" />
    <window $key_down.arrow_up="nav_up" />

    <div class="app">
      <section class="panel">
        <div class="pane-title">work bench</div>
        <div class="pane-content"><div id="cy" class="cy"></div></div>
        <div class="cli-box">
          <span class="cli-tag">ultracode</span>
          <span class="cli-prompt">&gt;</span> <span class="cli-ph">cli</span>
        </div>
        <div class="statusline">
          <span class="toggle">View: Board</span>
          <span class="toggle">Types: Threads</span>
        </div>
      </section>

      <section class="panel">
        <div class="pane-title">agent chat{%if @selected} · {@selected}{/if}</div>

        <div class="pane-content chat">
          {%for m <- @messages}
            <div class={"msg msg-" <> m.kind}>{m.text}</div>
          {/for}
          {%if @messages == []}
            <div class="placeholder">no activity</div>
          {/if}
        </div>

        <div class="cli-box">
          <span class="cli-tag">ultracode</span>
          <span class="cli-prompt">&gt;</span> <span class="cli-ph">cli</span>
        </div>

        <div class="picker">
          {%for a <- @agents}
            <div class={if a.uuid == @selected do "pick-row sel" else "pick-row" end} $click={:select_agent, uuid: a.uuid}>
              <span class={if a.online do "agent-dot on" else "agent-dot off" end}></span>
              <span class="pick-name">{a.uuid}</span>
              {%if a.focus_str != ""}<span class="pick-snip">{a.focus_str}</span>{/if}
              <span class="pick-tok">{a.ctx_str} ctx</span>
            </div>
          {/for}
        </div>

        <div class="statusline">
          <span class="badge">auto</span>
          <span>auto mode on · {length(@agents)} agents</span>
          <span class="status-tok">{@fleet.context} ctx · {@fleet.total} all-time</span>
        </div>
      </section>
    </div>
    """
  end
end
