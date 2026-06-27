defmodule LodestarWeb.PageController do
  use LodestarWeb, :controller

  def home(conn, _params) do
    render(conn, :home)
  end
end
