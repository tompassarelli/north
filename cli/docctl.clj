(ns docctl
  (:require [babashka.process :as process]
            [clojure.edn :as edn]
            [clojure.java.io :as io]
            [clojure.string :as str])
  (:import (java.math BigInteger)
           (java.nio.file Files Path StandardCopyOption)
           (java.security MessageDigest)
           (java.time Instant)))

(def manifest-suffix ".doc.edn")
(def ignored-directories #{".git" ".direnv" ".store" ".docctl" "node_modules" "out" "target" "build"})

(defn fail! [message]
  (binding [*out* *err*]
    (println (str "docctl: " message)))
  (System/exit 2))

(defn absolute-path [path]
  (.getAbsolutePath (io/file path)))

(defn git-root []
  (try
    (let [result (process/shell {:out :string :err :string} "git" "rev-parse" "--show-toplevel")]
      (when (zero? (:exit result))
        (str/trim (:out result))))
    (catch Exception _ nil)))

(defn root-for [path]
  (absolute-path (or path (System/getenv "DOCCTL_ROOT") (git-root) (System/getProperty "user.dir"))))

(defn relative-path [^String root ^String path]
  (str (.relativize (.toPath (io/file root)) (.toPath (io/file path)))))

(defn sha256-file [path]
  (let [digest (MessageDigest/getInstance "SHA-256")]
    (with-open [input (io/input-stream path)]
      (let [buffer (byte-array 65536)]
        (loop []
          (let [n (.read input buffer)]
            (when (pos? n)
              (.update digest buffer 0 n)
              (recur))))))
    (format "%064x" (BigInteger. 1 (.digest digest)))))

(defn sha256-string [value]
  (let [digest (MessageDigest/getInstance "SHA-256")]
    (.update digest (.getBytes (str value) "UTF-8"))
    (format "%064x" (BigInteger. 1 (.digest digest)))))

(defn git-revision [root]
  (try
    (let [result (process/shell {:out :string :err :string :dir root} "git" "rev-parse" "HEAD")]
      (when (zero? (:exit result)) (str/trim (:out result))))
    (catch Exception _ nil)))

(defn manifest-files [root]
  (letfn [(walk [^java.io.File directory]
            (mapcat (fn [^java.io.File child]
                      (cond
                        (.isDirectory child)
                        (if (contains? ignored-directories (.getName child)) [] (walk child))
                        (str/ends-with? (.getName child) manifest-suffix) [child]
                        :else []))
                    (or (.listFiles directory) [])))]
    (sort-by #(.getPath ^java.io.File %) (walk (io/file root)))))

(defn read-edn [path]
  (try
    (edn/read-string (slurp path))
    (catch Exception e
      {:docctl/error (str "invalid EDN: " (.getMessage e))})))

(defn document-path [manifest-path]
  (subs manifest-path 0 (- (count manifest-path) (count manifest-suffix))))

(defn source-entry [entry]
  (cond
    (string? entry) {:path entry}
    (map? entry) (when (string? (:path entry)) (select-keys entry [:path :revision]))
    :else nil))

(defn manifest-record [root manifest-file revision]
  (let [manifest-path (.getPath ^java.io.File manifest-file)
        manifest (read-edn manifest-path)
        document (relative-path root (document-path manifest-path))
        raw-sources (when (map? manifest) (:sources manifest))
        sources (mapv source-entry (or raw-sources []))
        errors (vec (concat
                     (when (:docctl/error manifest) [(:docctl/error manifest)])
                     (when-not (map? manifest) ["manifest must contain an EDN map"])
                     (when (and (map? manifest) (not (vector? raw-sources))) [":sources must be a vector"])
                     (when (and (vector? raw-sources) (some nil? sources)) ["each source must be a path string or map with :path"])))
        source-results (mapv (fn [{:keys [path revision]}]
                               (let [file (io/file root path)]
                                 {:path path
                                  :revision (or revision "content")
                                  :exists (.isFile file)
                                  :digest (when (.isFile file) (sha256-file file))}))
                             (filter some? sources))
        digest-map (or (:source-digests manifest) {})
        expected-digest (fn [path] (or (get digest-map path) (get digest-map (keyword path))))
        missing-digests (vec (keep (fn [{:keys [path exists]}]
                                     (when (and exists (nil? (expected-digest path))) path)) source-results))
        mismatches (vec (keep (fn [{:keys [path exists digest]}]
                                (cond
                                  (not exists) {:path path :reason :missing-source}
                                  (and (some? (expected-digest path)) (not= digest (expected-digest path)))
                                  {:path path :reason :changed}
                                  :else nil)) source-results))
        kind (:kind manifest)
        policy (or (:refresh-policy manifest) :manual)
        status (cond
                 (seq errors) :invalid
                 (= kind :archived) :archived
                 (or (empty? source-results) (seq missing-digests)) :unverified
                 (seq mismatches) :stale
                 :else :current)]
    {:document document
     :manifest (relative-path root manifest-path)
     :kind kind
     :refresh-policy policy
     :status status
     :revision revision
     :verified-at (:verified-at manifest)
     :sources source-results
     :missing-digests missing-digests
     :mismatches mismatches
     :errors errors
     :action (cond
               (= status :invalid) :repair-manifest
               (= status :archived) :none
               (= kind :generated) :regenerate
               (= policy :manual) :review
               (= status :stale) :review
               (= status :unverified) :establish-baseline
               :else :none)}))

(defn scan-root [root]
  (let [root (root-for root)
        revision (git-revision root)]
    {:root root
     :revision revision
     :manifests (mapv #(manifest-record root % revision) (manifest-files root))}))

(defn queue-worthy? [record]
  (and (#{:stale :unverified :invalid} (:status record))
       (not= :archived (:status record))))

(defn state-directory [root explicit]
  (absolute-path (or explicit
                     (System/getenv "DOCCTL_STATE_DIR")
                     (str (or (System/getenv "XDG_STATE_HOME")
                              (str (System/getProperty "user.home") "/.local/state"))
                          "/docctl/" (subs (sha256-string root) 0 16)))))

(defn queue-path [root explicit]
  (str (state-directory root explicit) "/queue.edn"))

(defn read-queue [path]
  (if (.isFile (io/file path))
    (let [value (read-edn path)] (if (vector? value) value []))
    []))

(defn write-atomic! [path value]
  (let [target (io/file path)
        _ (.mkdirs (.getParentFile target))
        temp (io/file (str path ".tmp-" (System/nanoTime)))]
    (spit temp (with-out-str (prn value)))
    (Files/move (.toPath temp) (.toPath target)
                (into-array StandardCopyOption [StandardCopyOption/REPLACE_EXISTING])))
  path)

(defn invalidate! [root explicit-state]
  (let [snapshot (scan-root root)
        path (queue-path (:root snapshot) explicit-state)
        now (str (Instant/now))
        additions (for [record (:manifests snapshot) :when (queue-worthy? record)]
                    {:document (:document record)
                     :manifest (:manifest record)
                     :status (:status record)
                     :action (:action record)
                     :policy (:refresh-policy record)
                     :mismatches (:mismatches record)
                     :detected-at now
                     :revision (:revision snapshot)})
        merged (->> additions
                    (reduce (fn [acc item] (assoc acc (:document item) item)) {})
                    vals
                    (sort-by :document)
                    vec)]
    (write-atomic! path merged)
    (assoc snapshot :queue path :queued merged)))

(defn print-value [value]
  (prn value)
  (flush))

(defn usage []
  (println "usage: docctl [--root PATH] [--state PATH] {scan|invalidate|queue}")
  (println "  scan        inspect manifests and report freshness (read-only)")
  (println "  invalidate  refresh the durable review queue")
  (println "  queue       print the current review queue"))

(defn parse-args [args]
  (loop [args args options {}]
    (if (empty? args)
      options
      (let [[head & tail] args]
        (case head
          "--root" (if (seq tail) (recur (next tail) (assoc options :root (first tail))) (fail! "--root needs a path"))
          "--state" (if (seq tail) (recur (next tail) (assoc options :state (first tail))) (fail! "--state needs a path"))
          (if (:command options)
            (fail! (str "unexpected argument: " head))
            (recur tail (assoc options :command head))))))))

(defn -main [& args]
  (let [{:keys [command root state]} (parse-args args)]
    (if-not command
      (do (usage) (System/exit 2))
      (case command
        "scan" (print-value (scan-root root))
        "invalidate" (print-value (select-keys (invalidate! root state) [:root :revision :queue :queued]))
        "queue" (let [resolved (root-for root)] (print-value {:root resolved :queue (queue-path resolved state) :queued (read-queue (queue-path resolved state))}))
        (do (usage) (System/exit 2))))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
