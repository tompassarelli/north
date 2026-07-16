import { query } from "@anthropic-ai/claude-agent-sdk";
import type { AgentProvider, ProviderAvailability } from "./types";
import { probeAnthropic } from "../provider-routing";

export const anthropicProvider: AgentProvider = {
  id: "anthropic",
  probe(): ProviderAvailability {
    return probeAnthropic();
  },
  query(args) {
    return query(args) as any;
  },
};
