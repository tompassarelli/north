export function run_northbridge_app_bang(options: {
  viewId?: string;
  sourceIdentity?: string;
}): Promise<unknown>;

export function handle_local_command_bang(
  runtime: unknown,
  ui: unknown,
  input: string,
): boolean;

export function palette_options(frame: string, query: string): Array<{ name: string }>;

export function parse_bridge_stream_bang(
  runtime: unknown,
  streamState: unknown,
  chunk: string,
): unknown;
