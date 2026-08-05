#!/usr/bin/env bb
;; Canonical publications must never turn omitted arguments into malformed
;; propositions. Explicit blank literals remain valid because several contracts
;; use them.
(require '[clojure.java.io :as io])

(def root (-> (io/file (System/getProperty "babashka.file"))
              .getParentFile .getParentFile .getParentFile .getPath))
(load-file (str root "/cli/coord.clj"))

(def fails (atom 0))
(defn check [label ok?]
  (println (str "  " (if ok? "PASS" "FAIL") " — " label))
  (when-not ok? (swap! fails inc)))
(defn invalid-write? [f]
  (try
    (f)
    false
    (catch clojure.lang.ExceptionInfo e
      (= :invalid-write (:type (ex-data e))))))

(check "publication rejects nil subject before creating a FRAMRPC client"
       (invalid-write?
        #(north.coord/publish!
          1 [{:op :assert :subject nil :predicate "note"
              :value "x" :cardinality :many}])))
(check "publication rejects blank subject before creating a FRAMRPC client"
       (invalid-write?
        #(north.coord/publish!
          1 [{:op :assert :subject " " :predicate "title"
              :value "x" :cardinality :one}])))
(check "publication rejects blank predicate before creating a FRAMRPC client"
       (invalid-write?
        #(north.coord/publish!
          1 [{:op :set :subject "@x" :predicate ""
              :values ["x"] :cardinality :one}])))
(check "publication rejects nil retract value before creating a FRAMRPC client"
       (invalid-write?
        #(north.coord/publish!
          1 [{:op :retract :subject "@x" :predicate "note" :value nil}])))
(check "explicit blank value remains valid"
       (= "" (north.coord/write-value! "@x" "note" "")))
(check "non-nil values retain string coercion"
       (= "42" (north.coord/write-value! "@x" "estimate" 42)))

(if (zero? @fails)
  (do (println "\ncoord write validation: ALL PASS") (System/exit 0))
  (do (println (str "\ncoord write validation: " @fails " FAIL")) (System/exit 1)))
