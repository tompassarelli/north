def unjudged(r):
    return r["outcome"] in ("running", "idle") or (r["outcome"] == "unclear" and not r["item"])


def succeeded(r, followed):
    return r["outcome"] == "done" and not followed and not (r["expects_landing"] and r["landed"] == 0)
