export class Keyword {
  constructor(readonly name: string) {}
}

export const kw = (name: string) => new Keyword(name);

export function coordPort(): number {
  return parseInt(process.env.NORTH_PORT ?? "7977", 10);
}

export function telemetryPartitionEnabled(): boolean {
  return process.env.NORTH_TELEMETRY_PARTITION === "1";
}

export function telemetryPort(): number {
  const value = parseInt(process.env.NORTH_TELEMETRY_PORT ?? "", 10);
  if (!Number.isInteger(value) || value < 1 || value > 65535) {
    throw new Error(
      "NORTH_TELEMETRY_PORT must be an integer from 1 through 65535 "
      + "when NORTH_TELEMETRY_PARTITION=1",
    );
  }
  return value;
}

export function storeSpaceId(): string {
  return process.env.BEAGLE_STORE_SPACE_ID || "north-coordination";
}

export function telemetrySpaceId(): string {
  return process.env.NORTH_TELEMETRY_SPACE_ID || "north-telemetry";
}

export function isTelemetrySubject(subject: string): boolean {
  return /^@(run|session|mine|guard_denial):/.test(subject);
}

export function nativeRouteForSubject(subject: string): { port: number; spaceId: string } {
  if (telemetryPartitionEnabled() && isTelemetrySubject(subject)) {
    return { port: telemetryPort(), spaceId: telemetrySpaceId() };
  }
  return { port: coordPort(), spaceId: storeSpaceId() };
}
