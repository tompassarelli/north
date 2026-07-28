#!/usr/bin/env bb
;; A failed spawn must carry the child's own last words.
;;
;; Observed 2026-07-29: a lane died and the operator was shown
;;   child exited before startup acknowledgement (exit 0); missing identity:
;;   kind,role,goal,provider,provider_target,live_input,... (16 fields)
;; — a list of symptoms — while the durable log held the single useful line,
;; `Connection refused` against the coordinator on :7977. failure-message
;; already appended a log tail; the tail was simply EMPTY when read, because
;; the child's stderr flushes as it exits and the parent read the instant it
;; noticed the exit. A race, not a missing feature.
(load-file (str (.getParent (java.io.File. (System/getProperty "babashka.file")))
                "/../spawn-process.clj"))
(in-ns 'north.spawn-process)
(clojure.core/refer-clojure)

(def failures (atom 0))
(def checks (atom 0))
(defn check! [label pass?]
  (swap! checks inc)
  (if pass? (println "PASS" label)
      (do (swap! failures inc) (println "FAIL" label))))

(def tmp (str "/tmp/spawn-settled-tail-" (System/nanoTime) ".log"))

;; --- the race this exists to close -----------------------------------------
(spit tmp "")
(future (Thread/sleep 250) (spit tmp "Connection refused"))
(check! "output written AFTER the read begins is still captured"
        (= "Connection refused" (settled-log-tail tmp)))

;; --- content already there costs nothing ------------------------------------
(spit tmp "Connection refused")
(let [t0 (System/currentTimeMillis)
      r (settled-log-tail tmp)]
  (check! "present content returns the tail" (= "Connection refused" r))
  (check! "present content does not wait" (< (- (System/currentTimeMillis) t0) 200)))

;; --- bounded: a genuinely silent child must not hang the spawn --------------
(spit tmp "")
(let [t0 (System/currentTimeMillis)
      r (settled-log-tail tmp)
      ms (- (System/currentTimeMillis) t0)]
  (check! "a permanently silent log gives up rather than hanging"
          (and (= "" r) (< ms 2500)))
  (check! "but it does wait before giving up" (>= ms 700)))

;; --- never throws: this runs on an ALREADY failing path ---------------------
(check! "missing file yields empty, no throw"
        (= "" (settled-log-tail "/tmp/north-no-such-spawn-log-xyz.log")))

(.delete (java.io.File. tmp))
(println (format "spawn-process-settled-tail: %d / %d PASS"
                 (- @checks @failures) @checks))
(System/exit (if (zero? @failures) 0 1))
