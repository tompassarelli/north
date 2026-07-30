#!/usr/bin/env bb
;; Rung 0 — the read primitives must never lie. Inject an error map, a malformed
;; envelope, and a transport timeout DIRECTLY at the coordinator primitives
;; (resolved / many / indexed-query / query-rows) and prove every injected failure yields a
;; TYPED, DISTINGUISHABLE throw — never a silent nil or empty collection that a
;; caller would read as "absent". Daemon-free: send-op is redefined per case, so
;; this is a pure contract test with no Fram checkout.
(require '[clojure.java.io :as io])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(load-file (str root "/cli/coord.clj"))

(def checks (atom []))
(defn check! [label value]
  (swap! checks conj [label (boolean value)]))

;; Run THUNK with send-op stubbed to REPLY (a value) or to THROW (when reply is
;; an ::throw marker). Returns {:threw? bool :type <ex-data :type or nil>
;;  :value <returned value or ::none>}.
(defn observe [reply-fn thunk]
  (with-redefs [north.coord/send-op (fn [_ op] (reply-fn op))]
    (try
      {:threw? false :value (thunk)}
      (catch clojure.lang.ExceptionInfo e
        {:threw? true :type (:type (ex-data e)) :data (ex-data e) :value ::none})
      (catch Exception e
        {:threw? true :type (class e) :data nil :value ::none}))))

(defn const [reply] (fn [_] reply))
(defn timeout [] (fn [_] (throw (ex-info "coordinator read timed out"
                                         {:type :coordinator-read-timeout}))))

;; The exclusive-success resolved envelope shapes.
(def valid-empty  {:value nil :members 0 :ambiguous? false :values [] :version 3})
(def valid-single {:value "v" :members 1 :ambiguous? false :values ["v"] :version 3})
(def valid-multi  {:value "a" :members 2 :ambiguous? true :values ["a" "b"] :version 3})

;; ---- resolved / many: honest empty is preserved --------------------------
(let [r (observe (const valid-empty) #(north.coord/resolved 1 "@x" "p"))]
  (check! "resolved on a genuinely-absent value returns nil (honest empty)"
          (and (not (:threw? r)) (nil? (:value r)))))
(let [r (observe (const valid-empty) #(north.coord/many 1 "@x" "p"))]
  (check! "many on a genuinely-absent value returns [] (honest empty)"
          (and (not (:threw? r)) (= [] (:value r)))))
(let [r (observe (const valid-single) #(north.coord/resolved 1 "@x" "p"))]
  (check! "resolved returns the single live value"
          (= "v" (:value r))))
(let [r (observe (const valid-multi) #(north.coord/many 1 "@x" "p"))]
  (check! "many returns every live value"
          (= ["a" "b"] (:value r))))

;; ---- resolved / many: every injected failure fails closed, never empty ----
(def resolved-failures
  {"an error map" (const {:error "no such op" :version 5})
   "a missing-keys envelope" (const {:value nil})
   "a value-not-in-values contradiction"
   (const {:value "x" :members 1 :ambiguous? false :values ["y"] :version 1})
   "a members/values count mismatch"
   (const {:value nil :members 2 :ambiguous? false :values ["a"] :version 1})
   "an ambiguous?/members disagreement"
   (const {:value "a" :members 2 :ambiguous? false :values ["a" "b"] :version 1})
   "a non-map response" (const [:ok])})

(doseq [[label reply-fn] resolved-failures]
  (let [rr (observe reply-fn #(north.coord/resolved 1 "@x" "p"))
        rm (observe reply-fn #(north.coord/many 1 "@x" "p"))]
    (check! (str "resolved rejects " label " with a typed throw, never nil")
            (and (:threw? rr) (= :malformed-resolved-response (:type rr))))
    (check! (str "many rejects " label " with a typed throw, never []")
            (and (:threw? rm) (= :malformed-resolved-response (:type rm))))))

;; A transport timeout is DISTINGUISHABLE from a malformed envelope: it
;; propagates the transport cause, not the malformed-envelope type — and it is
;; never swallowed into an empty read.
(let [rr (observe (timeout) #(north.coord/resolved 1 "@x" "p"))
      rm (observe (timeout) #(north.coord/many 1 "@x" "p"))]
  (check! "resolved surfaces a read timeout as a throw, not nil"
          (and (:threw? rr) (= :coordinator-read-timeout (:type rr))))
  (check! "many surfaces a read timeout as a throw, not []"
          (and (:threw? rm) (= :coordinator-read-timeout (:type rm))))
  (check! "timeout is distinguishable from a malformed envelope"
          (not= :malformed-resolved-response (:type rr))))

;; ---- show-rows: exact-subject fast path keeps the same honest envelope ----
(let [r (observe (const {:version 7
                         :rows [["owner" "personal"] ["title" "Thread"]]})
                 #(north.coord/show-rows 1 "@x"))]
  (check! "show-rows returns ordered exact-subject rows"
          (= [["owner" "personal"] ["title" "Thread"]] (:value r))))
(let [r (observe (const {:version 7 :rows []})
                 #(north.coord/show-rows 1 "@x"))]
  (check! "show-rows preserves an honest absent subject as []"
          (and (not (:threw? r)) (= [] (:value r)))))

(doseq [[label reply]
        [["an error map" {:error "unknown op"}]
         ["a missing version" {:rows []}]
         ["a torn row" {:version 1 :rows [["title"]]}]
         ["a non-string value" {:version 1 :rows [["title" 7]]}]]]
  (let [r (observe (const reply) #(north.coord/show-rows 1 "@x"))]
    (check! (str "show-rows rejects " label " with a typed throw, never []")
            (and (:threw? r) (= :malformed-show-response (:type r))))))

(let [r (observe (timeout) #(north.coord/show-rows 1 "@x"))]
  (check! "show-rows surfaces a read timeout as a throw, not []"
          (and (:threw? r) (= :coordinator-read-timeout (:type r)))))

;; ---- indexed-query: server aborts stay distinct from malformed envelopes --
(def indexed-query-fixture
  {:find "row"
   :rules [{:head {:rel "row" :args [{:var "e"}]}
            :body [{:rel "triple"
                    :args [{:var "e"} "kind" "rebuild-request"]}]}]})
(def query-time-limit-envelope
  {:error ["query evaluation stopped: query-time-limit"]
   :code :query-time-limit
   :timeout-ms 30000
   :version 7
   :engine "index"})

(let [r (observe (const query-time-limit-envelope)
                 #(north.coord/indexed-query 1 indexed-query-fixture 16))]
  (check! "indexed-query preserves a valid server evaluation timeout"
          (and (:threw? r)
               (= :indexed-query-error (:type r))
               (= :query-time-limit (get-in r [:data :code]))
               (= query-time-limit-envelope (get-in r [:data :response])))))

(let [r (observe
         (const (assoc query-time-limit-envelope :ok []))
         #(north.coord/indexed-query 1 indexed-query-fixture 16))]
  (check! "indexed-query keeps a contradictory success/error envelope malformed"
          (and (:threw? r)
               (= :malformed-indexed-query-response (:type r)))))

;; ---- query-rows / agg-rows: same discipline for :ok row reads -------------
(let [r (observe (const {:ok [] :version 1 :engine "scan"})
                 #(north.coord/query-rows 1 {:find "e" :rules []}))]
  (check! "query-rows on an empty result returns [] (honest empty)"
          (and (not (:threw? r)) (= [] (:value r)))))
(let [r (observe (const {:ok [["a"]] :version 1 :engine "scan"})
                 #(north.coord/query-rows 1 {:find "e" :rules []}))]
  (check! "query-rows returns the bound rows"
          (= [["a"]] (:value r))))

(def query-failures
  {"an error map" (const {:error ["boom"] :version 1})
   "a missing :ok envelope" (const {:version 1})
   "a non-vector :ok" (const {:ok nil :version 1})})

(doseq [[label reply-fn] query-failures]
  (let [r (observe reply-fn #(north.coord/query-rows 1 {:find "e" :rules []}))]
    (check! (str "query-rows rejects " label " with a typed throw, never []")
            (and (:threw? r) (= :malformed-query-response (:type r))))))

(let [r (observe (timeout) #(north.coord/query-rows 1 {:find "e" :rules []}))]
  (check! "query-rows surfaces a read timeout as a throw, not []"
          (and (:threw? r) (= :coordinator-read-timeout (:type r)))))

;; ---- report ---------------------------------------------------------------
(let [failed (remove second @checks)]
  (doseq [[label ok?] @checks]
    (println (str (if ok? "PASS " "FAIL ") label)))
  (if (seq failed)
    (do (println (str "coord read-envelope: " (count failed) " FAILED")) (System/exit 1))
    (println (str "coord read-envelope: PASS (" (count @checks) " checks)"))))
