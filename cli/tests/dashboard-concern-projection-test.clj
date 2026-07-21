#!/usr/bin/env bb
;; The dashboard consumes the STRICT VERSIONED concern machine projection, never the
;; human render. This drives dashboard-cli's `parse-concern-projection` directly:
;;   - a Kea-shaped fixture (11 orphaned + 72 retired) proves retired rows are
;;     excluded and by-repo grouping yields kea count 11 (not 72, not 83);
;;   - malformed / wrong-version payloads FAIL CLOSED to an error, never concerns.
(require '[babashka.process :as p]
         '[clojure.string :as str]
         '[cheshire.core :as json]
         '[clojure.java.io :as io])

(def test-script (or (System/getProperty "babashka.file") *file*))

;; dashboard-cli's library guard is process-environment based (its public entrypoint
;; is also the executable). Re-enter once with the guard set, then load it as a lib.
(when-not (= "1" (System/getenv "NORTH_DASHBOARD_LIB"))
  (let [result @(p/process ["env" "NORTH_DASHBOARD_LIB=1" "bb" test-script]
                           {:out :string :err :string})]
    (print (:out result))
    (binding [*out* *err*] (print (:err result)))
    (flush)
    (System/exit (:exit result))))

(def root (-> test-script io/file .getCanonicalFile .getParentFile .getParentFile .getParent str))
(let [dashboard-script (str root "/cli/dashboard-cli.clj")]
  (System/setProperty "babashka.file" dashboard-script)
  (try
    (load-file dashboard-script)
    (finally
      (System/setProperty "babashka.file" test-script))))

(def checks (atom []))
(defn check [label ok]
  (swap! checks conj [label ok])
  (println (if ok (str "PASS " label) (str "FAIL " label))))

;; Kea-shaped fixture: 11 orphaned + 72 retired concerns, all repo "kea". A consumer
;; that scraped rendered text (or failed to exclude retired) would over-count to 83
;; or mis-count to 72; the strict projection excludes retired and by-repo grouping
;; over the retained rows yields exactly 11.
(defn row [i cls retired?]
  {:id (str "@concern-1700000000000-kea" i) :agent (str "@kea" i)
   :repo "kea" :intent "kea work"
   :maturity (if retired? "building" "likely-to-land")
   :classification cls :online false :retired retired?})
(def kea-fixture
  (json/generate-string
   {:version 1
    :concerns (vec (concat (for [i (range 11)]  (row i "orphaned" false))
                           (for [i (range 72)]  (row (+ 100 i) "retired" true))))}))

(let [parsed  (parse-concern-projection kea-fixture)
      by-repo (->> (:concerns parsed) (group-by :repo)
                   (map (fn [[r cs]] [r (count cs)])) (into {}))]
  (check "Kea projection parses without error" (nil? (:err parsed)))
  (check "retired rows are excluded from the active projection"
         (= 11 (count (:concerns parsed))))
  (check "11 orphaned + 72 retired yields kea by-repo count 11"
         (= 11 (get by-repo "kea")))
  (check "every retained row is orphaned (orphaned counted, retired dropped)"
         (every? #(= "orphaned" (:classification %)) (:concerns parsed))))

;; Fail closed: a wrong-version or malformed payload must NOT render as concerns.
(check "wrong-version payload fails closed"
       (boolean (:err (parse-concern-projection
                       (json/generate-string {:version 2 :concerns []})))))
(check "non-JSON payload fails closed"
       (boolean (:err (parse-concern-projection "  @kea building kea {a b}  "))))
(check "non-object JSON payload fails closed"
       (boolean (:err (parse-concern-projection "[1,2,3]"))))
(check "concerns-not-a-list payload fails closed"
       (boolean (:err (parse-concern-projection
                       (json/generate-string {:version 1 :concerns {}})))))
(check "non-map projection row fails closed"
       (boolean (:err (parse-concern-projection
                       (json/generate-string {:version 1 :concerns ["nope"]})))))
(check "well-formed empty projection is not an error"
       (= [] (:concerns (parse-concern-projection
                         (json/generate-string {:version 1 :concerns []})))))

(let [failed (remove second @checks)]
  (println (str "dashboard concern projection: " (- (count @checks) (count failed))
                " / " (count @checks) " PASS"))
  (System/exit (if (empty? failed) 0 1)))
