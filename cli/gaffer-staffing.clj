;; One version-aware boundary for Gaffer's canonical staffing catalog.
;; Consumers use the normalized `presets` field regardless of whether a pinned
;; Gaffer release still carries the v1 `recipes` spelling or the v2 schema.
(ns north.gaffer-staffing
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]))

(defn catalog-path []
  (or (System/getenv "GAFFER_STAFFING_CATALOG")
      (str (or (System/getenv "GAFFER_HOME")
               (str (System/getProperty "user.home") "/code/gaffer"))
           "/staffing/catalog.json")))

(defn normalize-catalog [catalog path]
  (let [version (get catalog "version")
        presets (case version
                  1 (get catalog "recipes")
                  2 (get catalog "presets")
                  (throw (ex-info (str "unsupported Gaffer staffing catalog version "
                                       (pr-str version) " at " path)
                                  {:path path :version version})))]
    (when-not (and (map? (get catalog "defaults"))
                   (map? (get catalog "vocabulary"))
                   (vector? presets)
                   (vector? (get catalog "aliases")))
      (throw (ex-info (str "invalid Gaffer staffing catalog at " path)
                      {:path path :version version})))
    (let [names (mapv #(get % "name") presets)
          known (set names)]
      (when (or (some nil? names) (not= (count names) (count known)))
        (throw (ex-info (str "invalid or duplicate Gaffer preset name at " path)
                        {:path path :names names})))
      (doseq [alias (get catalog "aliases")]
        (when-not (and (string? (get alias "name"))
                       (contains? known (get alias "target")))
          (throw (ex-info (str "invalid Gaffer staffing alias at " path)
                          {:path path :alias alias}))))
      (-> catalog
          (dissoc "recipes")
          (assoc "presets" presets)))))

(defn load-catalog
  ([] (load-catalog (catalog-path)))
  ([path]
   (let [file (io/file path)]
     (normalize-catalog (json/parse-string (slurp file)) (.getPath file)))))

(defn presets-by-name [catalog]
  (into {} (map (juxt #(get % "name") identity) (get catalog "presets"))))
