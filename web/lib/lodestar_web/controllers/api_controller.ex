defmodule LodestarWeb.ApiController do
  use LodestarWeb, :controller

  # Cytoscape-ready thread DAG from the board daemon (:7977).
  def dag(conn, _params), do: json(conn, Lodestar.Threads.graph())

  # Live agent roster + fleet token totals from the agents daemon (:7978).
  def presence(conn, _params) do
    roster = Lodestar.Presence.roster()
    json(conn, %{agents: roster, fleet: Lodestar.Presence.fleet_tokens(roster)})
  end

  # Flat array in the shape wake's agents.wake `(entity agent …)` expects
  # (all string fields). This is what wake's `persist :feed` snapshots.
  def wake_presence(conn, _params) do
    rows =
      Lodestar.Presence.roster()
      |> Enum.map(fn r ->
        %{
          uuid: r.uuid,
          roles: r.roles_str,
          model: r.model_str,
          online: if(r.online, do: "online", else: "offline"),
          current_thread: r.current_thread || "",
          active_workflow: r.active_workflow || "",
          cost_usd: r.ctx_str
        }
      end)

    json(conn, rows)
  end

  # wake's sibling /live WebSocket — upgrade + hand off to LiveFeed (PubSub-fed).
  def live(conn, _params) do
    conn
    |> WebSockAdapter.upgrade(LodestarWeb.LiveFeed, [], timeout: 120_000)
    |> halt()
  end

  # The wake frontend shell: mounts the compiled wake bundle into #app, with
  # Cytoscape + the escape-hatch mounts. The wake app self-feeds via /presence
  # + /live. (Lives at /wake while we prove the stack; flips to / when ready.)
  @wake_shell """
  <!doctype html>
  <html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>lodestar</title>
    <link rel="stylesheet" href="/assets/css/app.css" />
    <style>html,body{margin:0;height:100%;background:#272e33;color:#d3c6aa;font-family:ui-sans-serif,system-ui,sans-serif}#app{height:100vh;overflow:auto}</style>
    <script src="/js/cytoscape.min.js"></script>
  </head>
  <body>
    <div id="app"></div>
    <script src="/js/wake-mounts.js"></script>
    <script src="/js/lodestar-ui.js"></script>
  </body>
  </html>
  """

  def wake_shell(conn, _params) do
    conn
    |> put_resp_content_type("text/html")
    |> send_resp(200, @wake_shell)
  end
end
