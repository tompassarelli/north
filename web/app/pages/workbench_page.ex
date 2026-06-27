defmodule LodestarWeb.WorkbenchPage do
  use Hologram.Page

  route "/"
  layout LodestarWeb.MainLayout

  def template do
    ~HOLO"""
    <div class="app">
      <section class="panel workarea">
        <div class="panel-label">workarea — thread DAG</div>
        <div class="panel-body">DAG workbench goes here</div>
        <div class="panel-cli">&gt; CLI: /view (graph/kanban)</div>
      </section>

      <section class="panel agents">
        <div class="panel-label">agents</div>
        <div class="panel-body">agent chat goes here</div>
        <div class="panel-cli">&gt; CLI</div>
      </section>
    </div>
    """
  end
end
