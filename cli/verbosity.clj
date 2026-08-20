;; North CLI verbosity — one dial, OTel's severity model, and a swallow helper
;; that stops discarded exceptions from being unrecoverable.
;;
;; WHY. The CLI holds 92 `(catch Exception _ nil)` sites. Most are correct: a
;; diagnostic that dies because a probe failed is worse than one that degrades.
;; But a swallowed exception leaves NO trace, so when the degraded path produces
;; a misleading answer there is nothing to read. That is not hypothetical — it
;; is how 2026-07-29 went:
;;
;;   - every `north tell` was refused with "coordinator unavailable"; the real
;;     cause was a log-identity mismatch, discarded three records down.
;;   - a failed spawn showed 16 missing identity fields; the durable log held
;;     `Connection refused`, which nothing surfaced.
;;
;; Each cost hours, and in both the process HAD the answer and threw it away.
;;
;; So: keep the swallow (the degraded path is right), but make the discarded
;; cause retrievable by turning one dial. `NORTH_LOG_LEVEL=DEBUG north <cmd>`
;; prints every swallowed exception with the context that named it.
;;
;; Levels are OTel's severity numbers, not a bespoke scale, so this stays
;; consistent with sdk/src/otel.ts and with anything that later reads the spans.
(ns north.verbosity
  (:require [clojure.string :as str]))

;; OTel severity numbers. The named level is the LOWEST number in its range.
(def SEVERITY
  {"TRACE" 1 "DEBUG" 5 "INFO" 9 "WARN" 13 "ERROR" 17 "FATAL" 21})

(def ^:private DEFAULT-SEVERITY 9) ; INFO

(defn configured-severity
  "Effective severity from the environment. NORTH_LOG_LEVEL beats
   OTEL_LOG_LEVEL; an unrecognised or blank value is INFO rather than an error —
   a bad log level must never be the reason a command fails."
  ([] (configured-severity
       {"NORTH_LOG_LEVEL" (System/getenv "NORTH_LOG_LEVEL")
        "OTEL_LOG_LEVEL" (System/getenv "OTEL_LOG_LEVEL")}))
  ([env]
   (let [raw (or (get env "NORTH_LOG_LEVEL") (get env "OTEL_LOG_LEVEL"))
         name (some-> raw str/trim str/upper-case)]
     (get SEVERITY name DEFAULT-SEVERITY))))

(defn enabled?
  "Would a message at `level` be emitted under the current dial?"
  ([level] (enabled? level (configured-severity)))
  ([level configured]
   (>= (get SEVERITY (str/upper-case (name level)) DEFAULT-SEVERITY) configured)))

(defn log!
  "Emit one line at `level` to stderr when the dial allows it. stderr, never
   stdout: these commands are piped and parsed, so diagnostics must not
   contaminate the data stream."
  [level message]
  (when (enabled? level)
    (binding [*out* *err*]
      (println (str "[" (str/upper-case (name level)) "] " message)))))

(defn attempt
  "Run `f`; on exception return `fallback` — and at DEBUG or finer, report the
   exception with `context` naming what was being attempted.

   Replaces the bare `(try (f) (catch Exception _ fallback))` shape. Identical
   behaviour at default verbosity; the difference is that the cause is now
   RETRIEVABLE instead of destroyed, without changing any caller's control flow.

   `context` should name the ATTEMPT, not the failure — 'reading coordinator
   facts', not 'facts read failed'. The exception supplies the failure."
  [context fallback f]
  (try
    (f)
    (catch Throwable t
      (log! :debug (str "swallowed while " context ": "
                        (.getName (class t))
                        (when-let [m (.getMessage t)] (str " — " m))))
      fallback)))
