#!/usr/bin/env bb
;; `north json show` must issue one exact :rpc/scan request. A whole-corpus scan
;; can remain functionally correct on tiny fixtures while exhausting managed
;; startup budgets on the live graph, so the binary wire shape is the contract.
(require '[babashka.classpath :as classpath]
         '[babashka.process :as proc]
         '[cheshire.core :as json]
         '[clojure.edn :as edn]
         '[clojure.java.io :as io])

(def root
  (.getCanonicalPath
   (io/file
    (or (System/getenv "NORTH_TEST_ROOT")
        (str (.getParent (io/file (System/getProperty "babashka.file")))
             "/../..")))))

(def fram
  (or
   (some-> (System/getenv "FRAM_TEST_CHECKOUT") io/file .getCanonicalPath)
   (some (fn [path]
           (let [candidate (io/file path)]
             (when (.isDirectory (io/file candidate "out"))
               (.getCanonicalPath candidate))))
         [(str root "/../fram") (str root "/../../fram/main")])
   (throw (ex-info "Fram head checkout is required; set FRAM_TEST_CHECKOUT"
                   {:root root}))))

(classpath/add-classpath (str fram "/out"))
(require '[framrpc :as wire]
         '[fram.types :as t])

(def checks (atom []))
(defn check! [label value]
  (swap! checks conj [label (boolean value)]))

(defn command-for [mode id]
  (case mode
    :cli
    ["timeout" "--kill-after=1s" "5s"
     (str root "/bin/north") "json" "show" id]

    :plain
    ["timeout" "--kill-after=1s" "5s"
     (str root "/bin/north") "show" id]

    :sdk
    ["timeout" "--kill-after=1s" "5s" "bun" "-e"
     (str "import { getThreadFacts } from "
          (json/generate-string (str root "/sdk/src/north-client.ts"))
          "; console.log(JSON.stringify(getThreadFacts("
          (json/generate-string id)
          ", { timeoutMs: 2000 })));")]

    :admission
    ["timeout" "--kill-after=1s" "10s" "bb" "-cp"
     (str root "/out:" fram "/out") "-e"
     (str "(System/setProperty \"north.agents.lib\" \"1\") "
          "(load-file " (pr-str (str root "/cli/agents-cli.clj")) ") "
          "(prn (select-keys (read-delegate-thread! " (pr-str id) ") "
          "[:id :title :committed?]))")]))

(defn read-exact! [input bytes offset length]
  (loop [position offset remaining length]
    (if (zero? remaining)
      true
      (let [read-count (.read input bytes position remaining)]
        (if (neg? read-count)
          false
          (recur (+ position read-count) (- remaining read-count)))))))

(defn read-request-frame! [input]
  (let [header (byte-array wire/rpc-v1-header-bytes)]
    (when-not (read-exact! input header 0 wire/rpc-v1-header-bytes)
      (throw (ex-info "FRAMRPC request ended inside its header"
                      {:type :rpc-truncated})))
    (let [buffer (doto (java.nio.ByteBuffer/wrap header)
                   (.order java.nio.ByteOrder/LITTLE_ENDIAN)
                   (.position 14))
          body-length (Integer/toUnsignedLong (.getInt buffer))]
      (when (> body-length wire/rpc-v1-max-body-bytes)
        (throw (ex-info "FRAMRPC request exceeds the body limit"
                        {:type :rpc-frame-too-large
                         :body-length body-length})))
      (let [body (byte-array (int body-length))
            frame (byte-array (+ wire/rpc-v1-header-bytes (int body-length)))]
        (when-not (read-exact! input body 0 (int body-length))
          (throw (ex-info "FRAMRPC request ended inside its body"
                          {:type :rpc-truncated})))
        (System/arraycopy header 0 frame 0 wire/rpc-v1-header-bytes)
        (System/arraycopy body 0 frame wire/rpc-v1-header-bytes
                          (int body-length))
        (wire/decode-rpc-frame-v1! frame)))))

(defn subject-of [id]
  (if (.startsWith ^String id "@") id (str "@" id)))

(defn response-for [frame expected-subject {:keys [version rows malformed?]}]
  (let [request (t/rpcframev1-request frame)
        operation (t/rpcrequest-op request)
        page (when (t/rpcrequest-page request)
               (wire/rpc-page-response! 0 nil true))
        payload
        (case operation
          :rpc/status
          (wire/rpc-status!
           :ready (count rows) :rpc/jvm
           (wire/rpc-record! :rpc/result-cache [0 0 0 0]))

          :rpc/scan
          (if malformed?
            (wire/rpc-record! :rpc/not-triples [])
            (wire/rpc-triples!
             (mapv (fn [[predicate value]]
                     (t/triple expected-subject predicate value))
                   rows)))

          wire/rpc-unit)
        error (when-not (contains? #{:rpc/status :rpc/scan} operation)
                (wire/rpc-error! :rpc/unsupported-operation false
                                 "fixture accepts only status and scan" nil))
        response (wire/rpc-response!
                  (t/rpcrequest-space request) operation version page error payload)]
    (wire/rpc-response-frame (t/rpcframev1-request-id frame) response)))

(defn serve-peer! [server expected-subject response requests worker-error]
  (try
    (loop []
      (with-open [socket (.accept server)]
        (let [frame (read-request-frame! (.getInputStream socket))
              request (t/rpcframev1-request frame)
              output (.getOutputStream socket)]
          (swap! requests conj request)
          (.write output
                  (wire/encode-rpc-frame-v1!
                   (response-for frame expected-subject response)))
          (.flush output)))
      (recur))
    (catch java.net.SocketException _)
    (catch Throwable error
      (reset! worker-error error))))

(defn scan-observations [requests]
  (mapv
   (fn [request]
     {:space (t/rpcrequest-space request)
      :pattern
      (mapv wire/rpc-option-value!
            (wire/rpc-record-fields!
             (t/rpc-request-payload-value request)
             :rpc/triple-pattern 3))})
   (filter #(= :rpc/scan (t/rpcrequest-op %)) requests)))

(defn exact-scan? [requests space subject]
  (= [{:space space :pattern [subject nil nil]}]
     (scan-observations requests)))

(defn invoke-peer
  [mode id response extra-env]
  (let [server (java.net.ServerSocket. 0)
        requests (atom [])
        worker-error (atom nil)
        subject (subject-of id)
        worker
        (future (serve-peer! server subject response requests worker-error))
        port (str (.getLocalPort server))
        routed-env
        (into {}
              (map (fn [[key value]]
                     [key (if (= "$SERVER_PORT" value) port value)]))
              extra-env)
        result
        (apply
         proc/shell
         {:continue true
          :out :string
          :err :string
          :extra-env
          (merge
           {"FRAM_HOME" fram
            "FRAM_BIN" (str fram "/bin")
            "FRAM_OUT" (str fram "/out")
            "FRAM_LOG" "/tmp/north-json-show-indexed-coordination.log"
            "FRAM_SERVER_CONNECT" "127.0.0.1"
            "FRAM_SERVER_PORT" port
            "FRAM_SPACE_ID" "north-coordination"
            "NORTH_FRAMRPC_HOST" "127.0.0.1"
            "NORTH_FRAMRPC_OUT" (str fram "/out")
            "NORTH_FRAMRPC_READ_TIMEOUT_MS" "2000"
            "NORTH_PORT" port
            "NORTH_TELEMETRY_SPACE_ID" "north-telemetry"
            "NORTH_TELEMETRY_PARTITION" "0"
            "NORTH_VERB_SLOTS" "0"
            "NORTH_AGENTS_LIB" "1"
            "NORTH_BIN" (str root "/bin/north")
            "NORTH_HOME" root
            "NO_COLOR" "1"
            "WORLD_MANIFEST_PATH" "/tmp/north-json-show-indexed-no-manifest"}
           routed-env)}
         (command-for mode id))
        _ (.close server)
        _ (try (deref worker 1000 nil) (catch Throwable _ nil))]
    {:result result :requests @requests :worker-error @worker-error}))

(let [id "019fb39e-94a9-7627-adc1-6b4dac07d834"
      rows [["kind" "thread"] ["title" "managed \"admission\""]]
      {:keys [result requests worker-error]}
      (invoke-peer :cli id {:version 17 :rows rows} {})
      parsed (when (zero? (:exit result))
               (json/parse-string (:out result) true))]
  (check! "exact JSON show exits successfully" (zero? (:exit result)))
  (check! "wrapper sends one exact-subject FRAMRPC scan"
          (and (nil? worker-error)
               (exact-scan? requests "north-coordination" (str "@" id))))
  (check! "exact rows retain the canonical JSON fact contract"
          (= [{:predicate "kind" :value "thread"}
              {:predicate "title" :value "managed \"admission\""}]
             parsed)))

(let [id "019fb39e-94a9-7627-adc1-6b4dac07d839"
      {:keys [result requests worker-error]}
      (invoke-peer :plain id {:version 17 :rows [["title" "Fast human show"]]} {})]
  (check! "human UUID show renders the indexed exact-subject result"
          (and (zero? (:exit result))
               (= "  title  Fast human show\n" (:out result))))
  (check! "human UUID show avoids a whole-corpus request"
          (and (nil? worker-error)
               (exact-scan? requests "north-coordination" (str "@" id)))))

(let [id "019fb39e-94a9-7627-adc1-6b4dac07d835"
      {:keys [result requests worker-error]}
      (invoke-peer :cli id {:version 18 :rows []} {})]
  (check! "authoritative absence remains an empty JSON array"
          (and (zero? (:exit result))
               (= [] (json/parse-string (:out result) true))
               (nil? worker-error)
               (exact-scan? requests "north-coordination" (str "@" id)))))

(let [telemetry-log "/tmp/north-json-show-indexed-telemetry.log"
      {:keys [result requests worker-error]}
      (invoke-peer
       :cli
       "run:indexed"
       {:version 19 :rows [["kind" "run"]]}
       {"NORTH_TELEMETRY_PARTITION" "1"
        "NORTH_TELEMETRY_PORT" "$SERVER_PORT"
        "FRAM_TELEMETRY_LOG" telemetry-log})]
  (check! "telemetry subjects retain Stage-A routing"
          (and (zero? (:exit result))
               (nil? worker-error)
               (exact-scan? requests "north-telemetry" "@run:indexed"))))

(let [directory
      (.toFile
       (java.nio.file.Files/createTempDirectory
        "north-framrpc-show-"
        (make-array java.nio.file.attribute.FileAttribute 0)))
      telemetry-log (.getCanonicalPath (io/file directory "telemetry.fifo"))]
  (try
    (proc/shell "mkfifo" telemetry-log)
    (let [{:keys [result requests worker-error]}
          (invoke-peer
           :plain
           "run:bounded"
           {:version 20 :rows [["kind" "run"] ["run_task" "bounded read"]]}
           {"NORTH_TELEMETRY_PARTITION" "1"
            "NORTH_TELEMETRY_PORT" "$SERVER_PORT"
            "FRAM_TELEMETRY_LOG" telemetry-log})]
      (check! "plain telemetry show renders exact rows without opening the origin log"
              (and (zero? (:exit result))
                   (= "  kind  run\n  run_task  bounded read\n" (:out result))))
      (check! "plain telemetry show retains the exact-subject FRAMRPC scan"
              (and (nil? worker-error)
                   (exact-scan? requests "north-telemetry" "@run:bounded"))))
    (finally
      (io/delete-file telemetry-log true)
      (io/delete-file directory true))))

(let [id "019fb39e-94a9-7627-adc1-6b4dac07d836"
      {:keys [result requests worker-error]}
      (invoke-peer :cli
                   id
                   {:version 21 :rows [] :malformed? true}
                   {})]
  (check! "malformed FRAMRPC scan payloads fail closed"
          (and (not (zero? (:exit result)))
               (nil? worker-error)
               (exact-scan? requests "north-coordination" (str "@" id))
               (re-find #"(?i)rpc|fram" (:err result))
               (empty? (:out result)))))

(let [id "019fb39e-94a9-7627-adc1-6b4dac07d837"
      rows [["kind" "thread"] ["title" "SDK admission"]]
      {:keys [result requests worker-error]}
      (invoke-peer :sdk id {:version 22 :rows rows} {})]
  (check! "SDK getThreadFacts consumes the exact wrapper projection"
          (and (zero? (:exit result))
               (= [{:predicate "kind" :value "thread"}
                   {:predicate "title" :value "SDK admission"}]
                  (json/parse-string (:out result) true))))
  (check! "SDK admission emits one exact-subject FRAMRPC scan"
          (and (nil? worker-error)
               (exact-scan? requests "north-coordination" (str "@" id)))))

(let [id "019fb39e-94a9-7627-adc1-6b4dac07d838"
      rows [["title" "Delegate admission"]
            ["kind" "thread"]
            ["committed" "2026-07-30"]
            ["done_when" "probe exits 0"]]
      {:keys [result requests worker-error]}
      (invoke-peer :admission id {:version 23 :rows rows} {})
      parsed (when (zero? (:exit result))
               (edn/read-string (:out result)))]
  (check! "delegate intake accepts a title-bearing exact projection"
          (and (zero? (:exit result))
               (= {:id id :title "Delegate admission" :committed? true}
                  parsed)))
  (check! "delegate intake reaches the same indexed FRAMRPC boundary"
          (and (nil? worker-error)
               (exact-scan? requests "north-coordination" (str "@" id)))))

(let [failed (remove second @checks)]
  (doseq [[label ok] @checks]
    (println (if ok "PASS" "FAIL") label))
  (println (format "json-show-indexed: %d / %d PASS"
                   (- (count @checks) (count failed))
                   (count @checks)))
  (System/exit (if (seq failed) 1 0)))
