export interface RoutedQueryArguments {
  input: string | Record<string, unknown>;
  options: Record<string, unknown>;
  writer: Record<string, unknown>;
  artifacts?: Record<string, unknown>;
  eventCommitter?: Record<string, unknown>;
}

export type RoutedQueryWithRegistryFn = ((arg0: Record<string, unknown>, arg1: RoutedQueryArguments, arg2: string | null, arg3: Record<string, unknown>) => Record<string, unknown>) | ((arg0: Record<string, unknown>, arg1: RoutedQueryArguments, arg2: string | null, arg3: Record<string, unknown>, arg4: Record<string, unknown> | null) => Record<string, unknown>) | ((arg0: Record<string, unknown>, arg1: RoutedQueryArguments, arg2: string | null, arg3: Record<string, unknown>, arg4: Record<string, unknown> | null, arg5: Record<string, unknown> | null) => Record<string, unknown>) | ((arg0: Record<string, unknown>, arg1: RoutedQueryArguments, arg2: string | null, arg3: Record<string, unknown>, arg4: Record<string, unknown> | null, arg5: Record<string, unknown> | null, arg6: Record<string, unknown> | null) => Record<string, unknown>);

export type WireQueryInput = string | Record<string, unknown>;

export declare function routedQueryWithRegistry(arg0: Record<string, unknown>, arg1: RoutedQueryArguments, arg2: string | null, arg3: Record<string, unknown>): Record<string, unknown>;
export declare function routedQueryWithRegistry(arg0: Record<string, unknown>, arg1: RoutedQueryArguments, arg2: string | null, arg3: Record<string, unknown>, arg4: Record<string, unknown> | null): Record<string, unknown>;
export declare function routedQueryWithRegistry(arg0: Record<string, unknown>, arg1: RoutedQueryArguments, arg2: string | null, arg3: Record<string, unknown>, arg4: Record<string, unknown> | null, arg5: Record<string, unknown> | null): Record<string, unknown>;
export declare function routedQueryWithRegistry(arg0: Record<string, unknown>, arg1: RoutedQueryArguments, arg2: string | null, arg3: Record<string, unknown>, arg4: Record<string, unknown> | null, arg5: Record<string, unknown> | null, arg6: Record<string, unknown> | null): Record<string, unknown>;
