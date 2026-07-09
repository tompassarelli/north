defmodule NorthWeb.PageController do
  use NorthWeb, :controller

  def home(conn, _params) do
    render(conn, :home)
  end
end
