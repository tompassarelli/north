#!/usr/bin/env bb
(require '[babashka.classpath :as classpath]
         '[clojure.java.io :as io])

(def root
  (.getCanonicalPath
   (io/file
    (or (System/getenv "NORTH_TEST_ROOT")
        (str (.getParent (io/file (System/getProperty "babashka.file")))
             "/../..")))))
(def store
  (.getCanonicalPath
   (io/file (or (System/getenv "BEAGLE_STORE_TEST_CHECKOUT")
                (System/getenv "BEAGLE_STORE_HOME")
                "/home/tom/code/beagle/main/store"))))

(classpath/add-classpath (str store "/out"))
(load-file (str root "/cli/json-search-cli.clj"))

(def facts
  [["@b" "title" "Beta"]
   ["@b" "note" "alpha second"]
   ["@a" "title" "Alpha"]
   ["@a" "alpha_state" "enabled"]
   ["@a" "note" "unrelated"]
   ["@c" "title" "Gamma"]
   ["@c" "note" "ALPHA third"]
   ["@c" "alpha_key" "yes"]])

(def results (search-rows facts "aLpHa"))
(def checks
  [["literal search is case-insensitive across title, predicate, and value"
    (= [["a" "alpha_state" "enabled"]
        ["b" "note" "alpha second"]
        ["c" "alpha_key" "yes"]
        ["a" "title" "Alpha"]
        ["c" "note" "ALPHA third"]]
       (mapv (juxt :subject :predicate :value) results))]
   ["the first round is deterministic and subject-diverse"
    (= ["a" "b" "c"] (mapv :subject (take 3 results)))]
   ["each result carries its subject title"
    (= ["Alpha" "Beta" "Gamma"] (mapv :title (take 3 results)))]
   ["regex punctuation is interpreted literally"
    (= [{:subject "literal" :title "Dot" :predicate "note" :value "has.a.dot"}]
       (search-rows [["@literal" "title" "Dot"]
                     ["@literal" "note" "has.a.dot"]
                     ["@other" "note" "anything"]]
                    "."))]
   ["no matches is an exact empty result"
    (= [] (search-rows facts "absent"))]
   ["an incomplete graph projection is refused"
    (try
      (with-redefs [north.coord/live-facts-view!
                    (fn [_] {:complete false :unavailable ["telemetry"] :facts []})]
        (search-results 7977 "alpha")
        false)
      (catch Exception error
        (= :incomplete-live-projection (:type (ex-data error)))))]])

(let [failures (remove second checks)]
  (doseq [[label ok] checks]
    (println (if ok "  [PASS] " "  [FAIL] ") label))
  (if (empty? failures)
    (println "json-search: all checks passed")
    (System/exit 1)))
