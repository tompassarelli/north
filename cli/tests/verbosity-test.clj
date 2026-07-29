#!/usr/bin/env bb
;; The verbosity dial must (a) default to silence, (b) never change control
;; flow, and (c) make a swallowed cause retrievable. If any of those slips the
;; feature is worse than nothing: a diagnostic that alters behaviour when you
;; turn it on cannot be trusted to diagnose.
(load-file (str (.getParent (java.io.File. (System/getProperty "babashka.file")))
                "/../verbosity.clj"))
(require '[clojure.string :as str])
(alias 'v 'north.verbosity)

(def failures (atom 0))
(def checks (atom 0))
(defn check! [label pass?]
  (swap! checks inc)
  (if pass? (println "PASS" label)
      (do (swap! failures inc) (println "FAIL" label))))

;; --- level resolution -------------------------------------------------------
(check! "defaults to INFO with no env"
        (= 9 (v/configured-severity {})))
(check! "NORTH_LOG_LEVEL wins over OTEL_LOG_LEVEL"
        (= 17 (v/configured-severity {"OTEL_LOG_LEVEL" "DEBUG"
                                      "NORTH_LOG_LEVEL" "ERROR"})))
(check! "OTEL_LOG_LEVEL is honoured on its own"
        (= 5 (v/configured-severity {"OTEL_LOG_LEVEL" "debug"})))
(check! "case-insensitive"
        (= 1 (v/configured-severity {"NORTH_LOG_LEVEL" "trace"})))

;; A bad log level must never be why a command fails.
(check! "unrecognised level falls back to INFO"
        (= 9 (v/configured-severity {"NORTH_LOG_LEVEL" "chatty"})))
(check! "blank level falls back to INFO"
        (= 9 (v/configured-severity {"NORTH_LOG_LEVEL" "   "})))

;; --- gating -----------------------------------------------------------------
(check! "INFO is enabled at INFO" (v/enabled? :info 9))
(check! "DEBUG is NOT enabled at INFO" (not (v/enabled? :debug 9)))
(check! "DEBUG is enabled at DEBUG" (v/enabled? :debug 5))
(check! "ERROR is enabled even at FATAL-only? no" (not (v/enabled? :error 21)))
(check! "FATAL is enabled at every level" (v/enabled? :fatal 21))

;; --- attempt: control flow is IDENTICAL regardless of verbosity -------------
(check! "attempt returns the value on success"
        (= 42 (v/attempt "computing" :fallback (fn [] 42))))
(check! "attempt returns the fallback on throw"
        (= :fallback (v/attempt "computing" :fallback
                                (fn [] (throw (ex-info "boom" {}))))))
(check! "attempt swallows Throwable, not merely Exception"
        (= :fallback (v/attempt "computing" :fallback
                                (fn [] (throw (AssertionError. "hard"))))))

;; --- attempt: silent by default, explanatory on demand ----------------------
(let [quiet (with-out-str
              (binding [*err* *out*]
                (v/attempt "reading coordinator facts" nil
                           (fn [] (throw (ex-info "connection refused" {}))))))]
  (check! "silent at default verbosity" (= "" quiet)))

(let [loud (with-out-str
             (binding [*err* *out*]
               (with-redefs [v/configured-severity (constantly 5)]
                 (v/attempt "reading coordinator facts" nil
                            (fn [] (throw (ex-info "connection refused" {})))))))]
  (check! "at DEBUG, reports the CONTEXT of the attempt"
          (str/includes? loud "reading coordinator facts"))
  (check! "at DEBUG, reports the exception MESSAGE"
          (str/includes? loud "connection refused"))
  (check! "at DEBUG, reports the exception TYPE"
          (str/includes? loud "ExceptionInfo")))

;; Diagnostics must not contaminate a parsed stdout stream.
(let [out (with-out-str
            (with-redefs [v/configured-severity (constantly 5)]
              (v/attempt "x" nil (fn [] (throw (ex-info "boom" {}))))))]
  (check! "diagnostics go to stderr, never stdout" (= "" out)))

(println (format "verbosity: %d / %d PASS" (- @checks @failures) @checks))
(System/exit (if (zero? @failures) 0 1))
