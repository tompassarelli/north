#!/usr/bin/env bb
;; The JSON fast path for the whole-corpus :facts read.
;;
;; Measured 2026-07-29 against the live coordinator, same corpus (347,972
;; triples), same process, alternating: EDN 10,595/10,971 ms, JSON 2,202/5,099
;; ms — 2.95x mean, with `=` between the two fact vectors. Decoding, not
;; transfer, is the cost.
;;
;; Two things must hold for that speedup to be safe, and both are tested here:
;;   1. The formats agree. A faster decode that accepted a DIFFERENT corpus
;;      would be a correctness bug wearing a benchmark as a disguise.
;;   2. The fast path degrades to EDN, never to "unavailable". A daemon
;;      predating :fmt answers a reject map; north's live view must not vanish
;;      because it guessed the dialect wrong. Version skew between an installed
;;      north and a running fram is the NORMAL state during a cutover.
;;
;; Daemon-free: fetch-triples is redefined per case, so this is a pure contract
;; test with no coordinator and no Fram checkout.
(require '[clojure.java.io :as io])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(load-file (str root "/cli/coord.clj"))

(def checks (atom []))
(defn check! [label value] (swap! checks conj [label (boolean value)]))

(def normalize (deref #'north.coord/normalize-facts-response))
(def valid (deref #'north.coord/valid-triples))
(def live-triples-at (deref #'north.coord/live-triples-at))

(def TRIPLES [["@a" "p" "v"] ["@b" "q" "w"]])

;; --- 1. the two wire formats describe the SAME value ------------------------
;; Verified against the live daemon: EDN answers with keyword keys, JSON with
;; string keys (it renders even `:code` keywords as strings).
(let [edn-shape {:version 7 :facts TRIPLES}
      json-shape {"version" 7 "facts" TRIPLES}]
  (check! "keyword-keyed and string-keyed responses normalise identically"
          (= (normalize edn-shape) (normalize json-shape)))
  (check! "normalised shape carries the version" (= 7 (:version (normalize json-shape))))
  (check! "normalised shape carries the facts" (= TRIPLES (:facts (normalize json-shape)))))

(check! "a non-map response normalises to nil, never a half-built shape"
        (nil? (normalize ["not" "a" "map"])))

;; --- 2. the triple contract stays exact across decoders ---------------------
;; A JSON decoder may hand back any sequential; coercion must not weaken the
;; per-element contract.
(check! "sequential rows are coerced to vectors"
        (= TRIPLES (valid (list (list "@a" "p" "v") (list "@b" "q" "w")))))
(check! "vectors pass through unchanged" (= TRIPLES (valid TRIPLES)))
(check! "an empty corpus is valid, not malformed" (= [] (valid [])))
(check! "a 2-element row is rejected" (nil? (valid [["@a" "p"]])))
(check! "a 4-element row is rejected" (nil? (valid [["@a" "p" "v" "x"]])))
(check! "a non-string member is rejected" (nil? (valid [["@a" "p" 3]])))
(check! "a non-sequential body is rejected" (nil? (valid "facts")))
(check! "nil facts are rejected" (nil? (valid nil)))

;; --- 3. the fast path is actually TAKEN -------------------------------------
;; Without this the whole change could be inert and every other test would still
;; pass — the precedent being a dispatch test that passed with the wiring
;; deleted.
(let [asked (atom [])]
  (with-redefs [north.coord/json-response-available? (constantly true)
                north.coord/fetch-triples
                (fn [_ _ json?] (swap! asked conj json?) {:version 7 :facts TRIPLES})]
    (let [result (live-triples-at 7977 "/log")]
      (check! "a JSON-capable classpath asks for JSON" (= [true] @asked))
      (check! "the JSON answer is returned" (:available result))
      (check! "no second round trip when JSON succeeds" (= 1 (count @asked))))))

(let [asked (atom [])]
  (with-redefs [north.coord/json-response-available? (constantly false)
                north.coord/fetch-triples
                (fn [_ _ json?] (swap! asked conj json?) {:version 7 :facts TRIPLES})]
    (live-triples-at 7977 "/log")
    (check! "a stdlib-only classpath asks for EDN and never for JSON"
            (= [false] @asked))))

;; --- 4. degradation is to EDN, never to unavailable -------------------------
;; The reject map a pre-:fmt daemon actually returns, verified on the wire:
;;   {"reject" [...], "code" "log-fence-required", ...}
(let [asked (atom [])]
  (with-redefs [north.coord/json-response-available? (constantly true)
                north.coord/fetch-triples
                (fn [_ _ json?]
                  (swap! asked conj json?)
                  (if json?
                    {"reject" ["unsupported"] "code" "bad-request"}
                    {:version 7 :facts TRIPLES}))]
    (let [result (live-triples-at 7977 "/log")]
      (check! "a malformed JSON answer retries as EDN" (= [true false] @asked))
      (check! "the EDN retry is what the caller sees" (:available result))
      (check! "the retried corpus is intact" (= TRIPLES (:facts result))))))

(let [asked (atom [])]
  (with-redefs [north.coord/json-response-available? (constantly true)
                north.coord/fetch-triples
                (fn [_ _ json?]
                  (swap! asked conj json?)
                  (if json?
                    (throw (ex-info "decoder blew up" {}))
                    {:version 7 :facts TRIPLES}))]
    (let [result (live-triples-at 7977 "/log")]
      (check! "a THROWN JSON failure also retries as EDN" (= [true false] @asked))
      (check! "a thrown JSON failure still yields the corpus" (:available result)))))

;; When BOTH formats fail the domain is genuinely unavailable, and the caller
;; must be told so rather than handed an empty corpus that reads as "no facts".
(with-redefs [north.coord/json-response-available? (constantly true)
              north.coord/fetch-triples
              (fn [_ _ _] (throw (ex-info "connection refused" {})))]
  (let [result (live-triples-at 7977 "/log")]
    (check! "both formats failing is unavailable" (false? (:available result)))
    (check! "the unavailable reason is preserved"
            (= "connection refused" (:error result)))
    (check! "an unavailable domain carries NO facts key that reads as empty"
            (nil? (:facts result)))))

;; A version-less answer is malformed even if the triples are perfect — the
;; version is what makes a read comparable to a write.
(with-redefs [north.coord/json-response-available? (constantly false)
              north.coord/fetch-triples (fn [_ _ _] {:facts TRIPLES})]
  (check! "facts without a version are malformed"
          (false? (:available (live-triples-at 7977 "/log")))))

;; --- 4b. the fast path must not escape the stubbing seam --------------------
;; The bug this pins actually shipped: the JSON path called send-envelope
;; directly, so every caller and test that injects a coordinator failure by
;; stubbing send-op-for-log was silently bypassed — the injected failure went to
;; a REAL socket instead. live-facts-view-detail-test dropped from 11/11 to
;; 5/11 and nothing else complained, because the tests that did pass were the
;; ones not exercising the seam. A faster path that is unobservable to the
;; failure-injection tests is not faster, it is untested.
(let [seen (atom [])]
  (with-redefs [north.coord/json-response-available? (constantly true)
                north.coord/send-op-for-log
                (fn [_ _ op & _] (swap! seen conj (:fmt op))
                  {:version 7 :facts TRIPLES})]
    (live-triples-at 7977 "/log")
    (check! "the JSON request goes through send-op-for-log, not around it"
            (= [:json] @seen))))

(let [seen (atom 0)]
  (with-redefs [north.coord/json-response-available? (constantly true)
                north.coord/send-op-for-log
                (fn [& _] (swap! seen inc) (throw (ex-info "injected" {})))]
    (let [result (live-triples-at 7977 "/log")]
      (check! "a failure injected at the seam still reaches BOTH attempts"
              (= 2 @seen))
      (check! "an injected seam failure surfaces as unavailable"
              (false? (:available result))))))

;; --- 5. the capability probe is honest --------------------------------------
(check! "this test classpath reports its real JSON capability"
        (boolean? (north.coord/json-response-available?)))
(with-redefs [north.coord/json-decoder (delay nil)]
  (check! "read-json-response! throws typed when no decoder exists"
          (= :coordinator-json-unavailable
             (try (north.coord/read-json-response! nil) nil
                  (catch clojure.lang.ExceptionInfo e (:type (ex-data e)))))))

(let [failed (remove second @checks)]
  (doseq [[label ok] @checks] (println (if ok "PASS" "FAIL") label))
  (println (format "coord-json-response: %d / %d PASS"
                   (- (count @checks) (count failed)) (count @checks)))
  (System/exit (if (seq failed) 1 0)))
