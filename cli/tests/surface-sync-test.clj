#!/usr/bin/env bb
;; Welds the CLI surface: committed share/help/ must equal a fresh regeneration
;; from cli/surface.edn, and the registry's :case verbs (+ aliases) must equal
;; bin/north's top-level case arms exactly, in both directions.

(require '[clojure.edn :as edn]
         '[clojure.string :as str]
         '[clojure.set :as set]
         '[babashka.fs :as fs]
         '[babashka.process :refer [shell]])

(def root (str (fs/parent (fs/parent (fs/parent (fs/absolutize *file*))))))
(def failures (atom []))
(defn fail! [msg] (swap! failures conj msg) (println (str "FAIL " msg)))
(defn pass [msg] (println (str "ok   " msg)))

;; -- 1. freshness: committed pages == regeneration ---------------------------
(let [tmp (str (fs/create-temp-dir))
      committed (str root "/share/help")]
  (shell {:out :string} "bb" (str root "/cli/surface-gen.clj") "--out" tmp)
  (let [names #(set (map fs/file-name (fs/list-dir %)))
        want (names tmp)
        have (if (fs/exists? committed) (names committed) #{})]
    (if (= want have)
      (pass (str "share/help file set (" (count want) " pages)"))
      (fail! (str "share/help file set drift — missing " (set/difference want have)
                  " stray " (set/difference have want))))
    (doseq [f (sort (set/intersection want have))]
      (if (= (slurp (str tmp "/" f)) (slurp (str committed "/" f)))
        (pass (str "share/help/" f " matches regeneration"))
        (fail! (str "share/help/" f " is stale — regenerate: bb cli/surface-gen.clj")))))
  (fs/delete-tree tmp))

;; -- 2. weld: registry :case verbs+aliases == bin/north case arms ------------
(def surface (edn/read-string (slurp (str root "/cli/surface.edn"))))
(def bin-src (slurp (str root "/bin/north")))

(def arm-tokens
  ;; Top-level arms sit at exactly two-space indent; nested cases are deeper.
  (->> (str/split-lines bin-src)
       (keep #(second (re-matches #"^  ([a-z\"*][A-Za-z0-9|_\"*-]*)\)(?:\s.*)?$" %)))
       (mapcat #(str/split % #"\|"))
       (remove (fn [t] (or (= t "\"\"") (str/starts-with? t "-"))))
       set))

(def registry-tokens
  (->> (:commands surface)
       (filter #(contains? #{nil :case} (:dispatch %)))
       (mapcat #(cons (:verb %) (:aliases %)))
       set))

(let [arms (disj arm-tokens "*")]
  (if (contains? arm-tokens "*")
    (pass "fram passthrough arm present")
    (fail! "no `*` passthrough arm in bin/north"))
  (if (str/includes? bin-src "\"panic\"")
    (pass "pre-case panic route present")
    (fail! "panic pre-case route missing from bin/north"))
  (if (= arms registry-tokens)
    (pass (str "case arms == registry (" (count arms) " verbs)"))
    (do (doseq [v (sort (set/difference arms registry-tokens))]
          (fail! (str "verb `" v "` is dispatched in bin/north but not in cli/surface.edn")))
        (doseq [v (sort (set/difference registry-tokens arms))]
          (fail! (str "verb `" v "` is registered in cli/surface.edn but has no bin/north case arm"))))))

(if (seq @failures)
  (do (println (str "surface-sync: " (count @failures) " failure(s)")) (System/exit 1))
  (println "surface-sync: passed"))
