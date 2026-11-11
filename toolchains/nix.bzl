def nix_tool(name):
    path = read_root_config("nix", name)
    if path == None:
        fail("buckconfig [nix] {} is unset: run buck2 inside the north dev shell, which links .buckconfig.flake".format(name))
    return path
