;; north-map.clj — deterministic batch registry + BARRIER fixture. Sibling to
;; presence-cli/msg-cli/lease-cli; speaks the SAME daemon wire (:assert / :version / :query /
;; :resolved). Managed lane creation moved to canonical North spawn/dispatch;
;; this legacy surface can only register deterministic test batches.
;;
;; Model (all facts, no new daemon verbs):
;;   @batch:<id>  batch_kind=fan-out  expected_count=N  barrier_k=K  role_template=..  task=..
;;                created_at=..  worker=<handle>          (one `worker` fact per spawned worker)
;;   @done:<id>:<worker>  done_batch=@batch:<id>  done_worker=<handle>  done_payload=..  done_at=..
;;
;; The BARRIER is the count-distinct QUORUM — coord.clj's first-class incremental
;; aggregate (the completion DUAL of mutual exclusion). distinct workers that emitted a
;; DONE against the batch. complete? := (count distinct-done-workers) >= K. Set semantics
;; collapse a worker that reports twice — the barrier counts WORKERS, not facts. Budget
;; (Sigma) is the SAME primitive with the sum reducer instead of count-distinct.
;;
;; usage:
;;   bb north-map.clj <port> map    <role-template> <N> <task> [K]   — fixture-only batch registration
;;   bb north-map.clj <port> done   <batch-id> <worker> <payload>    — a worker reports DONE (carries payload)
;;   bb north-map.clj <port> status <batch-id>                       — barrier state + aggregated payloads
;;   bb north-map.clj <port> barrier <batch-id>                      — just the derived fired? + done set
;;   bb north-map.clj <port> list                                    — known batches
;; env: MAP_SPAWN=0 is required and exists only for deterministic fixtures.
;;      Ambient/default execution fails closed toward canonical North spawn/dispatch.
(require '[clojure.java.io :as io] '[clojure.string :as str])

;; rec4: schema-validated structured output. Load the sibling validator so a batch can carry a JSON
;; schema (done_schema) and the `done` verb gates each worker's payload against it before accepting —
;; invalid => reject + retry, the K-of-N barrier does NOT advance. Sibling-relative; load-file leaves
;; the validator's own CLI dormant (main-guard).
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/schema-validate.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/topology-authority.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/gaffer-staffing.clj"))
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/batch-id.clj"))

;; shared coord substrate: cardinality-typed write verbs (move-C) live once in
;; cli/coord.clj. append! = MULTI coexist; put! = SINGLE last-writer-wins.
(load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
(def send-op    north.coord/send-op)
(def append!    north.coord/append!)
(def put!       north.coord/put!)
(def one        north.coord/resolved)
(def many       north.coord/many)
(def distinct-of north.coord/distinct-of)   ; count-distinct quorum, set form

(defn canonical-worker-preset [role]
  (let [path (or (System/getenv "GAFFER_STAFFING_CATALOG")
                 (str (or (System/getenv "GAFFER_HOME")
                          (str (System/getenv "HOME") "/code/gaffer"))
                      "/staffing/catalog.json"))]
    (try
      (let [catalog (north.gaffer-staffing/load-catalog path)
            preset (get (north.gaffer-staffing/presets-by-name catalog) role)
            route-problem (when preset
                            (north.gaffer-staffing/unsupported-route-problem
                             (get preset "tier") (get preset "deliberation")))]
        (cond
          (nil? preset) {:error (str "unknown Gaffer worker preset: " role)}
          (not= "worker" (get preset "topology"))
          {:error (str "north map requires a terminal worker preset; " role " is " (get preset "topology"))}
          route-problem {:error route-problem}
          :else {:preset preset}))
      (catch Exception e
        {:error (str "Gaffer staffing catalog unavailable: " path " (" (.getMessage e) ")")}))))

(defn q [port query] (:ok (send-op port {:op :query :query query})))

;; --- BARRIER = the count-distinct QUORUM (coord.clj's `distinct-of`), the
;; completion DUAL of mutual exclusion. The body binds every @done:* worker for
;; this batch; the shared aggregate folds them set-wise — a worker that reports
;; more than once counts ONCE. complete? := (count distinct-workers) >= K, i.e.
;; coord/quorum-met?; reusing the already-folded set below avoids a 2nd query.
(defn done-body [batch-e]
  [{:rel "triple" :args [{:var "d"} "done_batch"  batch-e]}
   {:rel "triple" :args [{:var "d"} "done_worker" {:var "w"}]}])

(defn done-workers [port batch-e]
  (distinct-of port ["w"] (done-body batch-e)))

;; aggregated DONE payloads: [worker payload] per reporting worker (for the synthesis step).
(defn done-payloads [port batch-e]
  (->> (q port {:find "dp"
                :rules [{:head {:rel "dp" :args [{:var "w"} {:var "pl"}]}
                         :body [{:rel "triple" :args [{:var "d"} "done_batch" batch-e]}
                                {:rel "triple" :args [{:var "d"} "done_worker" {:var "w"}]}
                                {:rel "triple" :args [{:var "d"} "done_payload" {:var "pl"}]}]}]})
       (reduce (fn [m [w pl]] (assoc m w pl)) {})))

(defn batch-meta [port batch-e]
  {:expected (some-> (one port batch-e "expected_count") Integer/parseInt)
   :k        (some-> (one port batch-e "barrier_k") Integer/parseInt)
   :role     (one port batch-e "role_template")
   :task     (one port batch-e "task")
   :workers  (many port batch-e "worker")})

(defn batch-ent [id] (if (str/starts-with? id "@batch:") id (str "@batch:" id)))

(let [[port verb & args] *command-line-args*
      port (Integer/parseInt port)]
  (case verb

    "map"          ; <role-template> <N> <task> [K] [<json-schema>]  — register @batch + fan out N workers
    (do
      (north.topology-authority/require-coordination! "map")
      (let [[tmpl n-s task k-s schema] args
          role-check (canonical-worker-preset tmpl)
          _ (when-let [problem (:error role-check)]
              (binding [*out* *err*] (println problem))
              (System/exit 1))
          n (Integer/parseInt n-s)
          k (if (and k-s (seq k-s)) (Integer/parseInt k-s) n)
          has-schema (and schema (seq (str/trim schema)))
          register-only? (= "0" (System/getenv "MAP_SPAWN"))
          _ (when-not register-only?
              (binding [*out* *err*]
                (println
                 "REJECTED: north map lane spawning is retired; use canonical north spawn/dispatch (MAP_SPAWN=0 is fixture-only registration)"))
              (System/exit 2))
          id (north.batch-id/fresh-id)
          e (batch-ent id)]
      (put! port e "batch_kind"     "fan-out")    ; single (batch metadata, write-once)
      (put! port e "expected_count" n)
      (put! port e "barrier_k"      k)
      (put! port e "role_template"  tmpl)
      (put! port e "task"           task)
      (put! port e "created_at"     (str (java.time.Instant/now)))
      (when has-schema (put! port e "done_schema" schema))   ; rec4: payload contract for every DONE
      (doseq [i (range 1 (inc n))]
        (let [slug (str tmpl "-" id "-" i)]
          (append! port e "worker" slug)))
      (println (str "batch " e "  N=" n " K=" k " role=" tmpl
                    (when has-schema "  +schema")
                    "  (fixture registration only, MAP_SPAWN=0)"))))

    "done"         ; <batch-id> <worker> <payload>  — a worker reports DONE against the batch
    (let [[id worker payload] args
          be (batch-ent id)
          de (str "@done:" (str/replace be #"^@batch:" "") ":" worker)
          ;; rec4 GATE: if the batch carries a schema, the payload must conform BEFORE we record the
          ;; DONE. A reject does NOT assert any @done:* fact, so the K-of-N barrier cannot advance on
          ;; malformed output — exit 3 + retry instruction is the agent's signal to re-run with a fix.
          ;; No schema => validate-json returns valid (:no-schema), identical to pre-rec4 behavior.
          schema (one port be "done_schema")
          vr (north.schema-validate/validate-json payload schema)]
      (when-not (:valid vr)
        (println (str "REJECTED  " worker " DONE -> " be "  (payload failed schema validation)"))
        (doseq [er (:errors vr)] (println (str "  - " er)))
        (println (str "  ↳ DONE not recorded; barrier unchanged. Fix the payload to satisfy the schema and "
                      "re-run:\n     bb <north-map.clj> " port " done " id " " worker " \"<conforming-json>\""))
        (System/exit 3))
      ;; @done:<batch>:<worker> is a per-(batch,worker) subject — these are single ON
      ;; THAT subject (write-once). The barrier's multi-ness is the AGGREGATE over many
      ;; @done:* entities (count-distinct done_worker), a read-side join — not a multi
      ;; group on one subject.
      (put! port de "done_batch"   be)            ; single
      (put! port de "done_worker"  worker)        ; single (per @done subject)
      (put! port de "done_payload" (or payload "")) ; single
      (put! port de "done_at"      (str (java.time.Instant/now)))  ; single
      (let [done (done-workers port be)
            {:keys [k expected]} (batch-meta port be)
            kk (or k expected)]
        (println (str worker " DONE -> " be "  (" (count done) "/" kk
                      (when expected (str " of " expected)) " barrier"
                      (if (and kk (>= (count done) kk)) " — FIRED" "") ")"))))

    ("status" "barrier")   ; <batch-id>  — derived K-of-N barrier state (+ payloads for status)
    (let [[id] args
          be (batch-ent id)
          {:keys [expected k role task workers]} (batch-meta port be)
          done (done-workers port be)
          kk (or k expected)
          fired (and kk (>= (count done) kk))]
      (println (format "%-14s %s" "batch" be))
      (when role (println (format "%-14s %s" "role" role)))
      (when (= verb "status") (when task (println (format "%-14s %s" "task" task))))
      (println (format "%-14s N=%s K=%s" "counts" (or expected "?") (or kk "?")))
      (println (format "%-14s %d  %s" "done" (count done) (str/join ", " (sort done))))
      (println (format "%-14s %s" "BARRIER" (if fired "FIRED (K-of-N satisfied)" "waiting")))
      (when (= verb "status")
        (println (format "%-14s %d/%s expected workers spawned" "membership"
                         (count (or workers [])) (or expected "?")))
        (when (seq done)
          (println "--- aggregated DONE payloads (synthesis input) ---")
          (doseq [[w pl] (sort (done-payloads port be))]
            (println (format "  %-24s %s" w pl)))))
      (System/exit (if fired 0 1)))   ; exit 0 ONLY when the barrier has fired — scriptable gate

    "list"
    (let [batches (->> (q port {:find "b"
                                :rules [{:head {:rel "b" :args [{:var "e"}]}
                                         :body [{:rel "triple" :args [{:var "e"} "batch_kind" "fan-out"]}]}]})
                       (map first) sort)]
      (println (format "%-40s %-6s %s" "BATCH" "K/N" "DONE"))
      (doseq [be batches]
        (let [{:keys [expected k]} (batch-meta port be)
              done (count (done-workers port be))]
          (println (format "%-40s %-6s %d%s" be (str (or k expected) "/" expected)
                           done (if (and k (>= done k)) "  FIRED" ""))))))

    "wait"   ; <batch-id> [timeout-secs=300]  — BLOCK until K-of-N done or deadline.
             ; Reaper: on timeout, assert barrier_status=timed_out, report missing
             ; workers to stderr, exit 4 — so a dead/silent worker SIGNALS instead of
             ; hanging the coordinator forever (closes the rec3 stranding vector).
    (let [[id ts] args
          be (batch-ent id)
          timeout-ms (* 1000 (Integer/parseInt (or ts "300")))
          {:keys [expected k workers]} (batch-meta port be)
          kk (or k expected)
          deadline (+ (System/currentTimeMillis) timeout-ms)]
      (loop []
        (let [done (done-workers port be)]
          (cond
            (and kk (>= (count done) kk))
            (do (println (format "BARRIER FIRED — %d/%s done: %s"
                                 (count done) kk (str/join ", " (sort done))))
                (System/exit 0))

            (>= (System/currentTimeMillis) deadline)
            (let [missing (remove (set done) (or workers []))]
              (put! port be "barrier_status" "timed_out")   ; single
              (binding [*out* *err*]
                (println (format "BARRIER TIMED OUT after %ss — %d/%s done; missing: %s"
                                 (quot timeout-ms 1000) (count done) (or kk "?")
                                 (if (seq missing) (str/join ", " missing) "(membership unknown)"))))
              (System/exit 4))

            :else
            (do (Thread/sleep 1000) (recur))))))

    (do (println "usage: north-map.clj <port> {map|done|status|barrier|wait|list}\n  map adds optional [K] [<json-schema>]; done is schema-gated when the batch carries one; wait blocks with a reaper timeout") (System/exit 2))))
