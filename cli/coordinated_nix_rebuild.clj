(ns north.coordinated-nix-rebuild
  (:require [babashka.process :as proc]
            [clojure.java.io :as io]
            [clojure.string :as str]))

(def rebuild-window-unit "north-rebuild-window")

(defn- owner-lock-path []
  (or (System/getenv "NORTH_REBUILD_OWNER_LOCK_PATH")
      (if-let [runtime-dir (System/getenv "XDG_RUNTIME_DIR")]
        (.getPath (io/file runtime-dir "north-rebuild-owner.lock"))
        (.getPath (io/file (System/getProperty "user.home")
                           ".cache" "north" "rebuild-owner.lock")))))

(defn with-owner-lock
  "Serialize the event owner and periodic fallback around plan + claim + launch."
  [f]
  (let [file (io/file (owner-lock-path))]
    (when-let [parent (.getParentFile file)] (.mkdirs parent))
    (with-open [random-access (java.io.RandomAccessFile. file "rw")
                channel (.getChannel random-access)]
      (let [lock (try (.tryLock channel) (catch Throwable _ nil))]
        (if lock
          (f)
          {:action "owner-busy" :count 0 :lock (.getPath file)})))))

(defn classify-window-unit-state [result]
  (let [reported (str/trim (str (:out result)))]
    (cond
      (#{"active" "activating" "deactivating" "reloading"} reported)
      {:state :active}

      (or (#{"inactive" "failed" "unknown"} reported)
          (contains? #{3 4} (:exit result)))
      {:state :inactive}

      :else
      {:state :unknown
       :reason (str/trim (str (:out result) (:err result)))})))

(defn window-unit-state []
  (try
    (classify-window-unit-state
     (proc/shell {:out :string :err :string :continue true}
                 "systemctl" "--user" "is-active" rebuild-window-unit))
    (catch Throwable error
      {:state :unknown :reason (or (.getMessage error) (str (class error)))})))

(defn launch-window! [north-bin window-id]
  (try
    (let [result
          (proc/shell {:out :string :err :string :continue true}
                      "systemd-run" "--user" "--collect"
                      (str "--unit=" rebuild-window-unit)
                      "--description=north coordinated rebuild window"
                      north-bin "rebuild" "run-window" window-id)]
      (if (zero? (:exit result))
        {:launched true :unit rebuild-window-unit}
        {:launched false
         :reason (str/trim (str (:out result) (:err result)))}))
    (catch Throwable error
      {:launched false :reason (or (.getMessage error) (str (class error)))})))

(defn collect-unlocked!
  "Plan and claim at most one window. The durable queue is unchanged on every
   pre-claim deferral, including an unavailable systemd wake path."
  [port dry? north-bin]
  (try
    (let [plan (north.rebuild-request/plan-window port)
          n (:count plan)
          queue-read (:queue-read plan)]
      (println (str "[coordinated-nix-rebuild-worker] queue"
                    " mode=" (:mode queue-read)
                    " bridge_start=" (:start-offset queue-read)
                    " bridge_end=" (:end-offset queue-read)
                    " bridge_target=" (:target-offset queue-read)
                    " bridge_bytes=" (:bytes-read queue-read)
                    " bridge_events=" (:relevant-events queue-read)
                    " corpus_queries=" (:corpus-queries queue-read)
                    " caught_up=" (:caught-up queue-read)))
      (case (:action plan)
        :idle {:action "idle" :count 0 :queue-read queue-read}
        :queued {:action "queued" :count n :queue-read queue-read}
        :waiting {:action "waiting" :count n :queue-read queue-read}
        :fire
        (if dry?
          (do
            (println (str "[coordinated-nix-rebuild-worker] WOULD open a rebuild window for "
                          n " request(s)"))
            {:action "would-fire" :count n :queue-read queue-read})
          (let [{:keys [state reason]} (window-unit-state)]
            (case state
              :active
              (do
                (println (str "[coordinated-nix-rebuild-worker] active window owns "
                              n " queued request(s)"))
                {:action "active" :count n :queue-read queue-read})

              :unknown
              (do
                (println (str "[coordinated-nix-rebuild-worker] wake deferred: "
                              (or reason "systemd state unavailable")))
                {:action "deferred" :count n :reason reason
                 :queue-read queue-read})

              :inactive
              (let [window-id
                    (north.rebuild-request/open-window!
                     port (mapv :id (:open plan)))
                    launch (launch-window! north-bin window-id)]
                (if (:launched launch)
                  (do
                    (println (str "[coordinated-nix-rebuild-worker] window " window-id
                                  " launched for " n " request(s) — unit "
                                  (:unit launch)))
                    {:action "fired" :count n :window window-id
                     :queue-read queue-read})
                  (do
                    (north.rebuild-request/set-window-action!
                     port window-id "deferred")
                    (println (str "[coordinated-nix-rebuild-worker] window " window-id
                                  " deferred: " (:reason launch)))
                    {:action "deferred" :count n :window window-id
                     :reason (:reason launch) :queue-read queue-read}))))))
        {:action "error" :count n
         :error (ex-info "unsupported rebuild window plan"
                         {:type :unsupported-rebuild-window-plan
                          :plan plan})}))
    (catch Throwable error
      (println (str "[coordinated-nix-rebuild-worker] error: " (.getMessage error)))
      {:action "error" :count 0 :error error})))

(defn collect! [port dry? north-bin]
  (with-owner-lock #(collect-unlocked! port dry? north-bin)))
