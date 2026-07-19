import type { Fact } from "./north-client";

export const JUDGMENT_GRADES = ["s", "m", "l"] as const;
export type JudgmentGrade = typeof JUDGMENT_GRADES[number];
export type JudgmentGradeStatus = "valid" | "unavailable" | "invalid";
export type JudgmentGradeSource = "thread" | "ad-hoc";

export interface JudgmentGradeSnapshot {
  /** Present only when status is valid. */
  readonly grade?: JudgmentGrade;
  readonly status: JudgmentGradeStatus;
  readonly source: JudgmentGradeSource;
}

function frozenSnapshot(snapshot: JudgmentGradeSnapshot): JudgmentGradeSnapshot {
  return Object.freeze(snapshot);
}

/** Canonical syntax is deliberately exact: no trimming, aliases, or case folding. */
export function parseJudgmentGrade(value: unknown): JudgmentGrade | undefined {
  return typeof value === "string" && (JUDGMENT_GRADES as readonly string[]).includes(value)
    ? value as JudgmentGrade
    : undefined;
}

export function requireJudgmentGrade(value: unknown): JudgmentGrade {
  const grade = parseJudgmentGrade(value);
  if (!grade) {
    throw new Error("judgment_grade must be exactly one of: s, m, l");
  }
  return grade;
}

/**
 * Freeze the dispatcher's grade from the admission-time thread projection.
 * Conflicting or malformed legacy values remain visible as invalid telemetry;
 * they are never normalized into a valid grade.
 */
export function judgmentGradeFromThreadFacts(
  facts: ReadonlyArray<Pick<Fact, "predicate" | "value">>,
): JudgmentGradeSnapshot {
  const values = facts
    .filter((fact) => fact.predicate === "judgment_grade")
    .map((fact) => fact.value);
  if (values.length === 0) {
    return frozenSnapshot({ status: "unavailable", source: "thread" });
  }
  if (values.length !== 1) {
    return frozenSnapshot({ status: "invalid", source: "thread" });
  }
  const grade = parseJudgmentGrade(values[0]);
  return grade
    ? frozenSnapshot({ grade, status: "valid", source: "thread" })
    : frozenSnapshot({ status: "invalid", source: "thread" });
}

export function adHocJudgmentGrade(): JudgmentGradeSnapshot {
  return frozenSnapshot({ status: "unavailable", source: "ad-hoc" });
}

if (import.meta.main) {
  const [command, value, ...extra] = process.argv.slice(2);
  if (command !== "validate" || value === undefined || extra.length) {
    console.error("usage: bun run judgment-grade.ts validate <s|m|l>");
    process.exit(2);
  }
  try {
    process.stdout.write(`${requireJudgmentGrade(value)}\n`);
  } catch (error) {
    console.error(error instanceof Error ? error.message : "invalid judgment_grade");
    process.exit(1);
  }
}
