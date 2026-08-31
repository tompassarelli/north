export interface AgentProvider {}

export interface AnthropicQueryRuntime {
  query: (arg0: Record<string, unknown>) => Record<string, unknown>;
  observe: (arg0: Record<string, unknown>, arg1: Record<string, unknown>) => Record<string, unknown>;
  createLifecycle: () => Record<string, unknown>;
  admit?: (arg0: Record<string, unknown>, arg1: Record<string, unknown> | null) => Promise<null>;
}

export interface WireQuery {}

export declare function admitAnthropic(arg0: Record<string, unknown>): Promise<null>;
export declare function admitAnthropic(arg0: Record<string, unknown>, arg1: Record<string, unknown> | null): Promise<null>;

export declare const anthropicProvider: Record<string, unknown>;

export declare function createAnthropicQuery(arg0: Record<string, unknown>, arg1: boolean): Record<string, unknown>;
export declare function createAnthropicQuery(arg0: Record<string, unknown>, arg1: boolean, arg2: Record<string, unknown>): Record<string, unknown>;

export declare function disposeAnthropicSdkQuery(arg0: Record<string, unknown> | null, arg1: Record<string, unknown> | null, arg2: Record<string, unknown> | null): Promise<null>;
export declare function disposeAnthropicSdkQuery(arg0: Record<string, unknown> | null, arg1: Record<string, unknown> | null, arg2: Record<string, unknown> | null, arg3: number | null): Promise<null>;
