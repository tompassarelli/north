;; orchestration-import-pointer-test.clj — the @catalog:current pointer-flip gate
;; (defect thread 019fb326; fix thread 019fb397).
;;
;; The regression this locks down: the ATOMIC FLIP is a put!, which supersedes ONLY
;; for a predicate declared `@<pred> cardinality single`; catalog_version was not,
;; so the flip APPENDED and consumers silently elected the stale first value.
;;
;; Daemon-free: load the importer as a library (main-guarded) and stub send-op.
;;   bb cli/tests/orchestration-import-pointer-test.clj
(require '[clojure.java.io :as io])

(def cli-dir (.getParentFile (io/file (System/getProperty "babashka.file"))))
(load-file (str (io/file (.getParentFile cli-dir) "orchestration-import-cli.clj")))

(def results (atom []))
(defn check [label pass?]
  (swap! results conj (boolean pass?))
  (println (format "  %s %s" (if pass? "✓" "✗") label)))

(defn ex-type [f]
  (try (f) ::no-throw (catch clojure.lang.ExceptionInfo e (:type (ex-data e)))))

;; A stub graph: {[subject predicate] [values]}. Only the shapes these helpers
;; issue are honoured; anything else is a test bug, not a silent empty.
(defn stub-op [graph-atom]
  (fn [_port op]
    (case (:op op)
      :query (let [[s p _] (get-in op [:query :rules 0 :body 0 :args])]
               {:ok (mapv vector (get @graph-atom [s p] []))})
      :assert (do (swap! graph-atom update [(:te op) (:p op)] (fnil conj []) (:r op))
                  {:ok 1})
      :retract (do (swap! graph-atom update [(:te op) (:p op)]
                          (fn [vs] (vec (remove #{(:r op)} vs))))
                   {:ok 1})
      (throw (ex-info (str "unstubbed op " (:op op)) {})))))

;; put!/retract! are bound to north.coord at load, so the socket path is only
;; closed off by stubbing the verbs the helpers call, not send-op alone. put!
;; models the engine's rule exactly: replace when the predicate is declared
;; single, append (idempotently) otherwise.
(defn stub-put [graph-atom]
  (fn [_port te p r]
    (if (= ["single"] (get @graph-atom [(str "@" p) "cardinality"]))
      (swap! graph-atom assoc [te p] [r])
      (swap! graph-atom update [te p] (fn [vs] (if (some #{r} vs) vs (conj (vec vs) r)))))
    {:ok 1}))
(defn stub-retract [graph-atom]
  (fn [_port te p r] ((stub-op graph-atom) _port {:op :retract :te te :p p :r r})))

(println "orchestration importer pointer flip — daemon-free stubs")

;; --- A. version parsing: `vN` is the spelling the surface prints -------------
(check "parse-version accepts a bare N"      (= 3 (parse-version "3")))
(check "parse-version accepts vN"            (= 3 (parse-version "v3")))
(check "parse-version is nil for no arg"     (nil? (parse-version nil)))
(check "parse-version throws on garbage"     (= :catalog-bad-version (ex-type #(parse-version "v3x"))))

(let [g (atom {["@catalog:current" "catalog_version"] ["7"]})]
  (with-redefs [send-op (stub-op g)]
    (check "version-arg falls back to the pointer with no arg" (= 7 (version-arg 0 nil)))
    (check "version-arg honours vN over the pointer"           (= 3 (version-arg 0 "v3")))))

;; --- B. the flip on an ALREADY-declared coordinator (the steady state) -------
;; put! supersedes, so one read confirms it and nothing is re-declared: a schema
;; write invalidates the coordinator's whole read-side cache.
(defn stub-resolved [graph-atom]
  (fn [_port te p]
    (let [vs (vec (get @graph-atom [te p] []))]
      {:value (first vs) :members (count vs) :ambiguous? (> (count vs) 1)
       :values vs :version 1})))

(let [g (atom {["@catalog:current" "catalog_version"] ["5"]
               ["@catalog_version" "cardinality"] ["single"]})]
  (with-redefs [put! (stub-put g) retract! (stub-retract g) resolved-envelope (stub-resolved g)]
    (flip! 0 6)
    (check "a declared pointer flips to exactly the new version"
           (= ["6"] (get @g ["@catalog:current" "catalog_version"])))
    (check "the steady-state flip does not re-declare cardinality"
           (= ["single"] (get @g ["@catalog_version" "cardinality"])))))

;; --- C. the flip on an UNDECLARED coordinator: verify, then repair in place ---
(let [g (atom {["@catalog:current" "catalog_version"] ["4"]})]
  (with-redefs [put! (stub-put g) retract! (stub-retract g) resolved-envelope (stub-resolved g)]
    (flip! 0 5)
    (check "an appending flip is repaired to exactly the new version"
           (= ["5"] (get @g ["@catalog:current" "catalog_version"])))
    (check "the repair declares @catalog_version cardinality single"
           (= ["single"] (get @g ["@catalog_version" "cardinality"])))))

;; The engine refuses a lossy multi->single, and that refusal must abort the import.
(let [g (atom {["@catalog:current" "catalog_version"] ["4"]})]
  (with-redefs [retract! (stub-retract g) resolved-envelope (stub-resolved g)
                put! (fn [_port te p r]
                       (if (= p "cardinality")
                         {:reject ["would collapse 1 live multi-valued group(s)"]}
                         ((stub-put g) 0 te p r)))]
    (check "a rejected declaration throws :catalog-cardinality-undeclared"
           (= :catalog-cardinality-undeclared (ex-type #(flip! 0 5))))))

;; A flip that still has not taken after the repair must fail LOUDLY.
(let [g (atom {})]
  (with-redefs [put! (fn [& _] {:ok 1}) retract! (fn [& _] {:ok 1})
                resolved-envelope (stub-resolved (atom {["@catalog:current" "catalog_version"] ["4" "5"]}))]
    (check "an unrepairable pointer throws :catalog-flip-not-atomic"
           (= :catalog-flip-not-atomic (ex-type #(flip! 0 5))))))

;; An unreadable coordinator must raise the READ failure, never a false non-flip.
(with-redefs [put! (fn [& _] {:ok 1})
              resolved-envelope (fn [& _] (throw (ex-info "boom" {:type :malformed-resolved-response})))]
  (check "an unreadable pointer surfaces the READ failure, not a false non-flip"
         (= :malformed-resolved-response (ex-type #(flip! 0 5)))))

(let [n (count @results) ok (count (filter true? @results))]
  (println (format "%s %d/%d" (if (= n ok) "PASS" "FAIL") ok n))
  (System/exit (if (= n ok) 0 1)))
