(require '[clojure.java.io :as io])

(let [root (.getParentFile (.getParentFile (io/file (System/getProperty "babashka.file"))))]
  (load-file (str root "/message-id.clj")))

(defn check [label ok]
  (println (format "  [%s] %s" (if ok "PASS" "FAIL") label))
  (when-not ok (System/exit 1)))

(let [now (java.time.LocalDateTime/of 2026 7 19 23 59 59)
      sender "same-sender"
      ids (mapv #(north.message-id/fresh-id
                  sender now (java.util.UUID. 0 (long %)))
                (range 10000))]
  (check "same sender/second produces 10,000 collision-free message ids"
         (= (count ids) (count (set ids))))
  (check "sortable timestamp prefix is stable and sender-independent"
         (and (every? #(.startsWith ^String % "20260719-235959-") ids)
              (= (north.message-id/fresh-id
                  "other-sender" now (java.util.UUID. 0 42))
                 (north.message-id/fresh-id
                  sender now (java.util.UUID. 0 42)))))
  (check "suffix carries a full canonical UUID"
         (every? #(re-matches
                    #"20260719-235959-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"
                    %)
                 ids)))

(println "message-id: 3 / 3 PASS")
