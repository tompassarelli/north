#!/usr/bin/env bb
;; A coordinator can be UP, serving the right log, and unable to do work.
;;
;; 2026-07-29: old-gen 99.98%, 2,715 full GCs, 1,704s of GC in a 69-minute
;; lifetime — 41% of its wall clock. `north show @swarm` took 57.8s on an IDLE
;; box after measuring 93ms earlier the same day. Every existing doctor probe
;; reported healthy throughout, because none of them asks whether the process
;; can still allocate. Finding it took hours.
(require '[babashka.process :as p] '[clojure.string :as str] '[clojure.java.io :as io])

(def test-script (or (System/getProperty "babashka.file") *file*))
(when-not (= "1" (System/getenv "NORTH_DASHBOARD_LIB"))
  (let [r @(p/process ["env" "NORTH_DASHBOARD_LIB=1" "bb" test-script]
                      {:out :string :err :string})]
    (print (:out r)) (binding [*out* *err*] (print (:err r))) (flush)
    (System/exit (:exit r))))

(def root (-> test-script io/file .getCanonicalFile .getParentFile .getParentFile .getParent str))
(let [dash (str root "/cli/dashboard-cli.clj")]
  (System/setProperty "babashka.file" dash)
  (try (load-file dash) (finally (System/setProperty "babashka.file" test-script))))

(def failures (atom 0))
(def checks (atom 0))
(defn check! [label pass?]
  (swap! checks inc)
  (if pass? (println "PASS" label)
      (do (swap! failures inc) (println "FAIL" label))))

(defn line-with [gc uptime]
  (with-redefs [coordinator-pid (constantly "999")
                jvm-gc-health (constantly gc)
                process-uptime-seconds (constantly uptime)]
    (coordinator-jvm-line "7977")))

;; --- the PARSE, against real jstat output -----------------------------------
;; Mocking jvm-gc-health leaves the column indices untested, and a wrong index is
;; silent: reading CGCT (11) instead of GCT (12) reported "0% of uptime in GC" on
;; a coordinator spending 41% of its life collecting. This is the exact output
;; from the live thrashing daemon on 2026-07-29.
(def REAL-GCUTIL
  (str "  S0     S1     E      O      M     CCS    YGC     YGCT     FGC    FGCT     CGC    CGCT       GCT   \n"
       "     -  91.71  29.49  99.98  81.93  73.79   5310    48.715  2715  1654.915   302     0.478  1704.109\n"))

(let [g (parse-gcutil REAL-GCUTIL)]
  (check! "parses old-gen from column 3" (= 99.98 (:old-pct g)))
  (check! "parses full-GC count from column 8" (= 2715 (:fgc g)))
  (check! "parses TOTAL GCT from column 12, not CGCT from 11"
          (= 1704.109 (:gc-seconds g)))
  (check! "does not mistake CGCT (0.478) for total GC time"
          (not= 0.478 (:gc-seconds g))))

(check! "a truncated jstat table yields nil rather than garbage"
        (nil? (parse-gcutil "S0 S1 E O\n1 2 3 4\n")))
(check! "empty output yields nil" (nil? (parse-gcutil "")))

;; --- the observed failure ---------------------------------------------------
(let [l (line-with {:old-pct 99.98 :fgc 2715 :gc-seconds 1704.1} 4140)]
  (check! "a thrashing JVM is an ERR" (str/includes? l "[ERR]"))
  (check! "names old-gen occupancy" (str/includes? l "old-gen 100.0%"))
  (check! "names the full-GC count" (str/includes? l "2715 full GCs"))
  (check! "names the share of uptime spent in GC" (str/includes? l "41% of uptime in GC"))
  (check! "names the paired-cutover remedy"
          (str/includes? l "sudo north-coord-runtime restart"))
  (check! "never recommends a direct service restart"
          (not (str/includes? l "systemctl restart north-coord.service"))))

;; --- a healthy JVM is quiet and non-failing ---------------------------------
(let [l (line-with {:old-pct 12.5 :fgc 3 :gc-seconds 1.2} 3600)]
  (check! "a healthy JVM is ok, not ERR"
          (and (str/includes? l "[ok]") (not (str/includes? l "[ERR]"))))
  (check! "a healthy JVM still reports its numbers" (str/includes? l "old-gen 12.5%")))

;; --- fault and observation thresholds are intentionally different ------------
;; Old-gen occupancy alone is noisy: a healthy JVM may retain most of its old
;; generation. It warrants observation, not a disruptive cutover. Sustained GC
;; time is the evidence that the process cannot allocate useful work.
(let [l (line-with {:old-pct 95.0 :fgc 1 :gc-seconds 0.5} 3600)]
  (check! "high old-gen alone is a warning" (str/includes? l "[warn]"))
  (check! "high old-gen alone is not an error" (not (str/includes? l "[ERR]")))
  (check! "high old-gen alone does not recommend a cutover"
          (not (str/includes? l "north-coord-runtime restart"))))
(check! "GC time alone above the threshold trips it"
        (str/includes? (line-with {:old-pct 40.0 :fgc 50 :gc-seconds 900.0} 3600) "[ERR]"))
(check! "a fresh JVM at observed old-gen occupancy stays ok"
        (str/includes? (line-with {:old-pct 83.7 :fgc 0 :gc-seconds 0.2} 120) "[ok]"))
(check! "just under both thresholds stays ok"
        (str/includes? (line-with {:old-pct 89.0 :fgc 10 :gc-seconds 600.0} 3600) "[ok]"))

;; --- singular/plural ---------------------------------------------------------
(check! "one full GC reads 'full GC', not 'full GCs'"
        (str/includes? (line-with {:old-pct 10.0 :fgc 1 :gc-seconds 0.1} 3600) "1 full GC ·"))

;; --- absence is never health ------------------------------------------------
;; The whole point is catching a daemon that looks fine. An unmeasurable JVM
;; must be visibly unmeasured, never rendered as ok.
(let [l (with-redefs [coordinator-pid (constantly nil)] (coordinator-jvm-line "7977"))]
  (check! "an unresolvable pid is not reported ok" (not (str/includes? l "[ok]")))
  (check! "an unresolvable pid says so" (str/includes? l "not resolvable")))

(let [l (with-redefs [coordinator-pid (constantly "999")
                      jvm-gc-health (constantly nil)]
          (coordinator-jvm-line "7977"))]
  (check! "missing jstat is not reported ok" (not (str/includes? l "[ok]")))
  (check! "missing jstat says why" (str/includes? l "unmeasurable")))

;; --- must never throw: it runs inside doctor --------------------------------
(check! "a zero uptime does not divide by zero"
        (string? (line-with {:old-pct 50.0 :fgc 1 :gc-seconds 1.0} 0)))

(println (format "coordinator-jvm-health: %d / %d PASS" (- @checks @failures) @checks))
(System/exit (if (zero? @failures) 0 1))
