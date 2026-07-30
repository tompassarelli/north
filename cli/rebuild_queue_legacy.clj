(ns north.rebuild-queue-legacy
  (:require [clojure.edn :as edn]
            [clojure.java.io :as io])
  (:import [java.nio ByteBuffer]
           [java.nio.charset CodingErrorAction StandardCharsets]
           [java.nio.file Files LinkOption]
           [java.nio.file.attribute BasicFileAttributes]))

(def max-batch-bytes (* 1024 1024))
(def queue-subject "@rebuild-queue")
(def queue-predicate "rebuild_queue")

(defn- canonical-regular-file [path]
  (let [file (io/file path)
        nio (.toPath file)]
    (when (Files/isSymbolicLink nio)
      (throw (ex-info "rebuild queue legacy log must not be a symbolic link"
                      {:type :legacy-log-invalid :path path})))
    (when-not (.isFile file)
      (throw (ex-info "rebuild queue legacy log is not a regular file"
                      {:type :legacy-log-invalid :path path})))
    (.getCanonicalPath file)))

(defn log-metadata [path]
  (let [canonical (canonical-regular-file path)
        nio (.toPath (io/file canonical))
        attrs (Files/readAttributes
               nio BasicFileAttributes
               (make-array LinkOption 0))
        file-key (.fileKey attrs)]
    (when-not file-key
      (throw (ex-info "rebuild queue legacy log has no stable file identity"
                      {:type :legacy-log-no-file-key :path canonical})))
    {:path canonical
     :file-key (str file-key)
     :length (.size attrs)}))

(defn validate-cursor! [{:keys [path file-key offset]} metadata]
  (when-not (= path (:path metadata))
    (throw (ex-info "rebuild queue legacy log path changed"
                    {:type :legacy-log-replaced
                     :expected path :actual (:path metadata)})))
  (when-not (= file-key (:file-key metadata))
    (throw (ex-info "rebuild queue legacy log identity changed"
                    {:type :legacy-log-replaced
                     :path path :expected file-key :actual (:file-key metadata)})))
  (when-not (and (integer? offset) (not (neg? offset)))
    (throw (ex-info "rebuild queue legacy cursor is invalid"
                    {:type :legacy-cursor-invalid :offset offset})))
  (when (> offset (:length metadata))
    (throw (ex-info "rebuild queue legacy log was truncated behind its cursor"
                    {:type :legacy-log-truncated
                     :path path :offset offset :length (:length metadata)})))
  metadata)

(defn cursor [metadata offset]
  {:path (:path metadata)
   :file-key (:file-key metadata)
   :offset offset})

(defn self-index-event? [record]
  (and (= queue-subject (:l record))
       (= queue-predicate (:p record))))

(defn relevant-event? [record]
  (let [subject (:l record)
        predicate (:p record)]
    (and (string? subject)
         (or (and (.startsWith subject "@rebuild-request:")
                  (#{"rebuild_request" "rebuild_request_satisfied"} predicate))
             (and (.startsWith subject "@rebuild-window:")
                  (= "window_action" predicate))))))

(defn- strict-utf8 [bytes length]
  (let [decoder (doto (.newDecoder StandardCharsets/UTF_8)
                  (.onMalformedInput CodingErrorAction/REPORT)
                  (.onUnmappableCharacter CodingErrorAction/REPORT))]
    (str (.decode decoder (ByteBuffer/wrap bytes 0 length)))))

(defn- last-newline-index [bytes]
  (loop [index (dec (alength bytes))]
    (cond
      (neg? index) nil
      (= 10 (bit-and 0xff (aget bytes index))) index
      :else (recur (dec index)))))

(defn read-batch
  "Read complete EDN log records from OFFSET toward the captured TARGET.
   Memory is bounded to one batch. A partial trailing record remains wholly
   unconsumed at its line-start cursor."
  [path offset target]
  (when-not (and (integer? offset) (integer? target)
                 (not (neg? offset)) (<= offset target))
    (throw (ex-info "invalid rebuild queue legacy read range"
                    {:type :legacy-read-range-invalid
                     :offset offset :target target})))
  (if (= offset target)
    {:records [] :next-offset offset :caught-up? true
     :partial-tail? false :non-self? false}
    (let [requested (int (min max-batch-bytes (- target offset)))
          bytes (byte-array requested)]
      (with-open [reader (java.io.RandomAccessFile. (io/file path) "r")]
        (.seek reader (long offset))
        (.readFully reader bytes))
      (if-let [newline-index (last-newline-index bytes)]
        (let [complete-bytes (inc newline-index)
              text (try
                     (strict-utf8 bytes complete-bytes)
                     (catch Exception error
                       (throw (ex-info "rebuild queue legacy log contains invalid UTF-8"
                                       {:type :legacy-log-invalid-utf8
                                        :path path :offset offset}
                                       error))))
              records
              (mapv
               (fn [line]
                 (try
                   (let [record (edn/read-string line)]
                     (when-not (map? record)
                       (throw (ex-info "legacy record is not a map" {})))
                     record)
                   (catch Exception error
                     (throw
                      (ex-info "rebuild queue legacy log contains malformed EDN"
                               {:type :legacy-log-malformed
                                :path path :offset offset}
                               error)))))
               (remove empty? (.split text "\n")))
              next-offset (+ offset complete-bytes)
              caught-up? (= next-offset target)]
          {:records records
           :next-offset next-offset
           :caught-up? caught-up?
           :partial-tail? (and (< next-offset target)
                               (= requested (- target offset)))
           :non-self? (boolean (some (complement self-index-event?) records))})
        (if (= requested (- target offset))
          {:records [] :next-offset offset :caught-up? false
           :partial-tail? true :non-self? false}
          (throw
           (ex-info "rebuild queue legacy log record exceeds the read bound"
                    {:type :legacy-log-line-too-long
                     :path path :offset offset
                     :max-bytes max-batch-bytes})))))))
