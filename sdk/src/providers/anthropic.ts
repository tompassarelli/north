import { query } from "@anthropic-ai/claude-agent-sdk";
import type { AgentProvider, ProviderAvailability } from "./types";
import { probeAnthropic } from "../provider-routing";
import { observeAnthropicQuery } from "./anthropic-observations";

export const anthropicProvider: AgentProvider = {
  id: "anthropic",
  probe(): ProviderAvailability {
    return probeAnthropic();
  },
  query(args) {
    return observeAnthropicQuery(query(args) as any);
  },
};
