#!/usr/bin/env bb
;; Regenerate store-rpc-golden-packets.json from Beagle Store's own encoder — the packets MUST
;; come from the server's wire authority, never from the SDK codec:
;;   bb -cp "$BEAGLE_STORE_OUT" sdk/test/fixtures/store-rpc-golden-packets.gen.clj \
;;     > sdk/test/fixtures/store-rpc-golden-packets.json
(require '[store.rpc :as w]
         '[store.types :as t]
         '[cheshire.core :as json])

(def space "north-coordination")
(def resource "managed-agent-write:8f2a")
(def holder "north-sdk-writer")

(defn b64 [^bytes bytes]
  (.encodeToString (java.util.Base64/getEncoder) bytes))

(defn req [id request] (b64 (w/store-rpc-encode-packet-v2! (w/store-rpc-request-packet id request))))
(defn resp [id response] (b64 (w/store-rpc-encode-packet-v2! (w/store-rpc-response-packet id response))))

(def fence (w/rpc-fence! resource holder 42))
(def transaction (t/transaction-coordinate space 46))

(defn nest [n]
  (loop [k n term "leaf"]
    (if (zero? k) term (recur (dec k) (t/triple term false true)))))

(def cursor
  (w/rpc-query-cursor!
   42 "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef" 1
   (w/rpc-query-row! [7 (t/triple "@agent:x" "role" "worker")])))

(def packets
  {"version-request"
   (req 1 (w/rpc-request! space :rpc/version nil nil nil w/rpc-unit))

   "scan-request-page1"
   (req 2 (w/rpc-request! space :rpc/scan nil
                          (w/rpc-page-request! 200 nil) nil
                          (w/rpc-triple-pattern! "@agent:x" nil nil)))

   "scan-request-page2"
   (req 3 (w/rpc-request! space :rpc/scan nil
                          (w/rpc-page-request! 200 cursor) nil
                          (w/rpc-triple-pattern! "@agent:x" nil nil)))

   "lease-acquire-request"
   (req 4 (w/rpc-request! space :rpc/lease-acquire 41 nil nil
                          (w/rpc-lease-acquire! resource holder 60000)))

   "lease-renew-request"
   (req 5 (w/rpc-request! space :rpc/lease-renew nil nil nil
                          (w/rpc-lease-renew! fence 60000)))

   "lease-release-request"
   (req 6 (w/rpc-request! space :rpc/lease-release nil nil nil fence))

   "lease-check-request"
   (req 7 (w/rpc-request! space :rpc/lease-check nil nil nil fence))

   "batch-request"
   (req 8 (w/rpc-request!
           space :rpc/batch 99 nil nil
           (w/rpc-batch!
            [(w/rpc-action! :rpc/assert (t/triple "@agent:x" "role" "worker")
                            w/rpc-subject-any)
             (w/rpc-action! :rpc/retract (t/triple "@agent:x" "kind" "lane")
                            w/rpc-subject-existing)]
            fence)))

   "batch-request-unfenced"
   (req 9 (w/rpc-request!
           space :rpc/batch nil nil nil
           (w/rpc-batch!
            [(w/rpc-action! :rpc/assert (t/triple "@agent:x" "goal" "naïve 😀 goal")
                            w/rpc-subject-any)]
            nil)))

   "deep-term-request-256"
   (req 10 (w/rpc-request! space :rpc/batch nil nil nil (nest 256)))

   "version-response"
   (resp 1 (w/rpc-response! space :rpc/version 42 nil nil w/rpc-unit))

   "scan-response-page"
   (resp 2 (w/rpc-response!
            space :rpc/scan 42
            (w/rpc-page-response! 0 cursor false) nil
            (w/rpc-triples! [(t/triple "@agent:x" "role" "worker")
                             (t/triple "@agent:x" "kind" "lane")])))

   "scan-response-final"
   (resp 3 (w/rpc-response! space :rpc/scan 42
                            (w/rpc-page-response! 1 nil true) nil
                            (w/rpc-triples! [])))

   "lease-grant-response"
   (resp 4 (w/rpc-response! space :rpc/lease-acquire 43 nil nil
                            (w/rpc-lease-grant!
                             fence (t/instant 1785000000 123456789))))

   "lease-released-response"
   (resp 6 (w/rpc-response! space :rpc/lease-release 45 nil nil
                            (w/rpc-lease-released! true)))

   "lease-check-response"
   (resp 7 (w/rpc-response! space :rpc/lease-check 45 nil nil
                            (w/rpc-lease-check!
                             true (t/instant 1785000060 0))))

   "lease-check-absent-response"
   (resp 7 (w/rpc-response! space :rpc/lease-check 45 nil nil
                            (w/rpc-lease-check! false nil)))

   "batch-response"
   (resp 8 (w/rpc-response!
            space :rpc/batch 46 nil nil
            (w/rpc-mutation-result!
             [(w/rpc-action-result!
               0 true (t/occurrence-coordinate transaction 0))
              (w/rpc-action-result!
               1 false (t/occurrence-coordinate transaction 1))])))

   "conflict-error-response"
   (resp 8 (w/rpc-response!
            space :rpc/batch 47 nil
            (w/rpc-error! :rpc/conflict true
                          "expected-version does not match current version" nil)
            nil))

   "lease-fence-mismatch-error-response"
   (resp 8 (w/rpc-response!
            space :rpc/batch 47 nil
            (w/rpc-error! :rpc/lease-fence-mismatch false
                          "lease fence does not name the current lease" nil)
            nil))

   "durability-ambiguous-error-response"
   (resp 8 (w/rpc-response!
            space :rpc/batch 47 nil
            (w/rpc-error! :durability-ambiguous true
                          "commit outcome is durability-ambiguous; restart is required"
                          (t/triple "detail" "restart" "required"))
            nil))

   "space-mismatch-error-response"
   (resp 1 (w/rpc-response!
            space :rpc/version 0 nil
            (w/rpc-error! :rpc/space-mismatch false
                          "request SpaceId does not match the served space" nil)
            nil))

   "lease-held-error-response"
   (resp 4 (w/rpc-response!
            space :rpc/lease-acquire 42 nil
            (w/rpc-error! :rpc/lease-held false
                          "lease resource is already held" nil)
            nil))

   "status-response"
   (resp 11 (w/rpc-response!
             space :rpc/status 42 nil nil
             (w/rpc-status! :serving 1234 :native
                            (w/rpc-record! :rpc/result-cache [1 2 3 4]))))

   "term-atoms-response"
   (resp 12 (w/rpc-response!
             space :rpc/scan 42 nil nil
             (w/rpc-triples!
              [(t/triple -9007199254740991 1.5 true)
               (t/triple false :rpc/unit "naïve 😀")
               (t/triple (t/instant 0 0) (t/instant -1 999999999) 0)])))})

(println (json/generate-string (into (sorted-map) packets) {:pretty true}))
