#!/usr/bin/env bb
;; Sanctioned pre-pass for the frozen North v0.3 logs. Epoch compaction preserves
;; domain state but does not preserve transaction order, and its header occupies
;; tx=0. FRAMLOG replay requires increasing coordinates beginning at one.
(require '[clojure.edn :as edn]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(import '[java.io FileOutputStream]
        '[java.nio.file Files StandardCopyOption]
        '[java.security MessageDigest])

(defn fail! [message data]
  (throw (ex-info message data)))

(defn sha256-bytes [^bytes bytes]
  (let [digest (.digest (MessageDigest/getInstance "SHA-256") bytes)]
    (apply str (map #(format "%02x" (bit-and (int %) 255)) digest))))

(defn sha256-file [path]
  (sha256-bytes (Files/readAllBytes (.toPath (io/file path)))))

(defn rows! [source]
  (with-open [reader (io/reader source)]
    (->> (line-seq reader)
         (map-indexed
          (fn [index line]
            (when (str/blank? line)
              (fail! "normalization source contains a blank row"
                     {:source source :line (inc index)}))
            (let [row (try (edn/read-string line)
                           (catch Throwable error
                             (fail! "normalization source contains malformed EDN"
                                    {:source source :line (inc index)
                                     :cause (.getMessage error)})))]
              (when-not (and (map? row) (integer? (:tx row))
                             (not (neg? (:tx row))))
                (fail! "normalization requires non-negative integer transactions"
                       {:source source :line (inc index) :row row}))
              [index row])))
         doall)))

(defn write-forced! [path indexed-rows]
  (with-open [file-out (FileOutputStream. (io/file path))
              writer (io/writer file-out)]
    (doseq [[_ row] indexed-rows]
      (.write writer (pr-str row))
      (.write writer "\n"))
    (.flush writer)
    (.force (.getChannel file-out) true)))

(defn existing-report [source target report-path]
  (when (or (.exists (io/file target)) (.exists (io/file report-path)))
    (when-not (and (.isFile (io/file target)) (.isFile (io/file report-path)))
      (fail! "normalization target and report must either both exist or both be absent"
             {:target target :report report-path}))
    (let [report (edn/read-string (slurp report-path))]
      (when-not (and (= (sha256-file source) (get-in report [:source :sha256]))
                     (= (sha256-file target) (get-in report [:output :sha256])))
        (fail! "existing normalization does not match its source or report"
               {:target target :report report-path}))
      report)))

(defn normalize! [source target]
  (let [source-file (.getCanonicalFile (io/file source))
        target-file (.getCanonicalFile (io/file target))
        report-file (io/file (str (.getPath target-file) ".normalization.edn"))]
    (when-not (.isFile source-file)
      (fail! "normalization source is not a readable file" {:source source}))
    (when (= source-file target-file)
      (fail! "normalization target must differ from source" {:source source}))
    (when-not (.isDirectory (.getParentFile target-file))
      (fail! "normalization target parent must already exist" {:target target}))
    (or (existing-report (.getPath source-file) (.getPath target-file)
                         (.getPath report-file))
        (let [indexed (rows! (.getPath source-file))
              txs (set (map (comp :tx second) indexed))
              zero-count (count (filter #(zero? (:tx (second %))) indexed))
              descents (count (filter (fn [[[ _ left] [_ right]]]
                                        (> (:tx left) (:tx right)))
                                      (partition 2 1 indexed)))]
          (when (and (pos? zero-count) (contains? txs 1))
            (fail! "tx=0 cannot be remapped because tx=1 already exists"
                   {:source source :tx-zero zero-count}))
          (let [normalized
                (->> indexed
                     (map (fn [[index row]]
                            [index (if (zero? (:tx row)) (assoc row :tx 1) row)]))
                     (sort-by (fn [[index row]] [(:tx row) index]))
                     vec)
                tmp (Files/createTempFile (.toPath (.getParentFile target-file))
                                          ".north-fram-normalize-" ".tmp"
                                          (make-array java.nio.file.attribute.FileAttribute 0))]
            (try
              (write-forced! (.toFile tmp) normalized)
              (Files/move tmp (.toPath target-file)
                          (into-array StandardCopyOption
                                      [StandardCopyOption/ATOMIC_MOVE]))
              (let [report
                    {:format :north/framlog-normalization-v1
                     :policy {:order :stable-transaction-then-source-line
                              :tx-zero :remap-to-one-only-when-one-absent}
                     :source {:path (.getPath source-file)
                              :bytes (.length source-file)
                              :sha256 (sha256-file source-file)}
                     :output {:path (.getPath target-file)
                              :bytes (.length target-file)
                              :sha256 (sha256-file target-file)}
                     :rows (count indexed)
                     :nonmonotonic-descents descents
                     :remapped-zero zero-count}]
                (spit report-file (str (pr-str report) "\n"))
                report)
              (catch Throwable error
                (Files/deleteIfExists tmp)
                (Files/deleteIfExists (.toPath target-file))
                (throw error))))))))

(try
  (when-not (= 2 (count *command-line-args*))
    (fail! "usage: framlog-normalize.clj SOURCE TARGET" {}))
  (prn (apply normalize! *command-line-args*))
  (catch Throwable error
    (binding [*out* *err*]
      (prn {:error (or (:type (ex-data error)) :normalization-failed)
            :message (.getMessage error) :data (ex-data error)}))
    (System/exit 1)))
