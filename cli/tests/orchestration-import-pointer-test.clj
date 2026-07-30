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
;; closed off by stubbing the verbs the helpers call, not send-op alone.
(defn stub-put [graph-atom]
  (fn [_port te p r] ((stub-op graph-atom) _port {:op :assert :te te :p p :r r})))
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

;; --- B. cardinality preflight ------------------------------------------------
(let [g (atom {["@catalog:current" "catalog_version"] ["4"]})]
  (with-redefs [send-op (stub-op g) put! (stub-put g) retract! (stub-retract g)]
    (check "undeclared catalog_version is not single" (false? (declared-single? 0 "catalog_version")))
    (ensure-pointer-single! 0)
    (check "preflight declares @catalog_version cardinality single"
           (= ["single"] (get @g ["@catalog_version" "cardinality"])))
    (check "declared-single? sees the declaration" (true? (declared-single? 0 "catalog_version")))))

;; An already-appended pointer must be collapsed to the HIGHEST version before the
;; declaration, since the engine refuses a lossy multi->single.
(let [g (atom {["@catalog:current" "catalog_version"] ["2" "3"]})]
  (with-redefs [send-op (stub-op g) put! (stub-put g) retract! (stub-retract g)]
    (ensure-pointer-single! 0)
    (check "preflight sheds the stale pointer value, keeping the highest"
           (= ["3"] (get @g ["@catalog:current" "catalog_version"])))))

;; A refused declaration must abort the import, never proceed to an appending flip.
(let [g (atom {})]
  (with-redefs [send-op (stub-op g)
                retract! (stub-retract g)
                put! (fn [& _] {:reject ["would collapse 1 live multi-valued group(s)"]})]
    (check "a rejected declaration throws :catalog-cardinality-undeclared"
           (= :catalog-cardinality-undeclared (ex-type #(ensure-pointer-single! 0))))))

;; --- C. the flip post-condition ---------------------------------------------
;; the real read is the STRICT resolved envelope, so an unreadable coordinator
;; raises its own typed failure instead of looking like a pointer that never flipped
(defn stub-resolved [vs]
  (fn [_port _te _p] {:value (first vs) :members (count vs) :ambiguous? (> (count vs) 1)
                      :values (vec vs) :version 1}))

(with-redefs [resolved-envelope (stub-resolved ["5"])]
  (check "assert-flip! passes on a single-valued pointer" (nil? (assert-flip! 0 5))))

(with-redefs [resolved-envelope (stub-resolved ["4" "5"])]
  (check "an appended pointer throws :catalog-flip-not-atomic"
         (= :catalog-flip-not-atomic (ex-type #(assert-flip! 0 5)))))

(with-redefs [resolved-envelope (fn [& _] (throw (ex-info "boom" {:type :malformed-resolved-response})))]
  (check "an unreadable pointer surfaces the READ failure, not a false non-flip"
         (= :malformed-resolved-response (ex-type #(assert-flip! 0 5)))))

;; --- D. a failed cardinality read never reads as "not declared" --------------
(with-redefs [send-op (fn [& _] {:error "query-time-limit"})]
  (check "an errored cardinality query throws :catalog-cardinality-unreadable"
         (= :catalog-cardinality-unreadable (ex-type #(declared-single? 0 "catalog_version")))))

(let [n (count @results) ok (count (filter true? @results))]
  (println (format "%s %d/%d" (if (= n ok) "PASS" "FAIL") ok n))
  (System/exit (if (= n ok) 0 1)))
