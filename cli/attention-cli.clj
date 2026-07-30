#!/usr/bin/env bb
;; Public CLI for durable follows and replayable notifications.
;; argv: <port> <follow|unfollow|following|notifications> ...
(require '[clojure.java.io :as io]
         '[clojure.string :as str])

(load-file
 (str (.getParent (io/file (System/getProperty "babashka.file"))) "/attention.clj"))

(def usage
  (str
   "usage:\n"
   "  north follow <thread> [--as <principal>] "
   "[--events progress,outcome,dependency,evidence,activity,changed] "
   "[--delivery inbox|notify]\n"
   "  north unfollow <thread> [--as <principal>]\n"
   "  north following [--as <principal>]\n"
   "  north notifications [--as <principal>] [--all] [--mark-read]"))

(defn usage-error! [message]
  (binding [*out* *err*]
    (println (str "attention: " message))
    (println usage))
  (System/exit 2))

(def value-options #{"--as" "--events" "--delivery"})
(def boolean-options #{"--all" "--mark-read"})
(def verb-option-allowlist
  {"follow" #{"--as" "--events" "--delivery"}
   "unfollow" #{"--as"}
   "following" #{"--as"}
   "notifications" #{"--as" "--all" "--mark-read"}})

(defn parse-options [args]
  (loop [remaining args
         positional []
         options {}]
    (if (empty? remaining)
      {:positional positional :options options}
      (let [token (first remaining)]
        (cond
          (contains? value-options token)
          (do
            (when (< (count remaining) 2)
              (usage-error! (str token " requires a value")))
            (when (contains? options token)
              (usage-error! (str token " may not repeat")))
            (recur (nnext remaining)
                   positional
                   (assoc options token (second remaining))))

          (contains? boolean-options token)
          (do
            (when (contains? options token)
              (usage-error! (str token " may not repeat")))
            (recur (next remaining) positional (assoc options token true)))

          (str/starts-with? token "--")
          (usage-error! (str "unknown option " token))

          :else
          (recur (next remaining) (conj positional token) options))))))

(defn selected-principal [port options]
  (let [candidate (or (get options "--as")
                      (north.attention/default-principal port))]
    (north.attention/require-principal! port candidate)))

(defn event-filters [options]
  (if-let [raw (get options "--events")]
    (->> (str/split raw #",")
         (map str/trim)
         (remove str/blank?)
         set)
    north.attention/default-event-filters))

(defn require-verb-options! [verb options]
  (when-not (contains? verb-option-allowlist verb)
    (usage-error! (str "unknown attention verb " (pr-str verb))))
  (let [allowed (get verb-option-allowlist verb)
        unsupported (->> (keys options) (remove allowed) sort vec)]
    (when (seq unsupported)
      (usage-error!
       (str verb " does not accept " (str/join ", " unsupported))))))

(defn print-following! [rows]
  (println
   (format "%-51s %-22s %-10s %-12s %s"
           "SUBSCRIPTION" "ABOUT" "DELIVERY" "OFFSET" "EVENTS"))
  (doseq [{:keys [id about delivery cursor-offset event-filters]} rows]
    (println
     (format "%-51s %-22s %-10s %-12s %s"
             id about delivery cursor-offset
             (str/join "," (sort event-filters))))))

(defn print-notifications! [rows]
  (println
   (format "%-51s %-18s %-24s %s"
           "NOTIFICATION" "KIND" "ABOUT" "SUBJECT"))
  (doseq [{:keys [id attention-kind about subject body]} rows]
    (println
     (format "%-51s %-18s %-24s %s"
             id attention-kind (or about "-") subject))
    (println (str "  " body))))

(let [[port-text verb & args] *command-line-args*
      port (when (and port-text (re-matches #"[0-9]+" port-text))
             (parse-long port-text))]
  (when-not (and port (<= 1 port 65535))
    (usage-error! "port must be an integer from 1 through 65535"))
  (let [{:keys [positional options]} (parse-options args)
        _ (require-verb-options! verb options)
        principal (selected-principal port options)]
    (case verb
      "follow"
      (do
        (when-not (= 1 (count positional))
          (usage-error! "follow requires exactly one thread"))
        (let [subscription
              (north.attention/follow!
               port
               {:principal principal
                :about (first positional)
                :event-filters (event-filters options)
                :delivery (or (get options "--delivery") "inbox")})]
          (println (str "following " (first positional)
                        " as " principal " via " subscription))))

      "unfollow"
      (do
        (when-not (= 1 (count positional))
          (usage-error! "unfollow requires exactly one thread"))
        (if-let [subscription
                 (north.attention/unfollow!
                  port {:principal principal :about (first positional)})]
          (println (str "unfollowed " (first positional)
                        " as " principal " via " subscription))
          (println (str "not following " (first positional)
                        " as " principal))))

      "following"
      (do
        (when (seq positional)
          (usage-error! "following accepts no positional arguments"))
        (print-following! (north.attention/following port principal)))

      "notifications"
      (do
        (when (seq positional)
          (usage-error! "notifications accepts no positional arguments"))
        (let [rows
              (north.attention/notifications
               port principal
               {:include-read? (boolean (get options "--all"))
                :mark-read? (boolean (get options "--mark-read"))})]
          (print-notifications! rows)
          (when (get options "--mark-read")
            (println (str "marked " (count rows) " displayed notification(s) read")))))

      (usage-error! (str "unknown attention verb " (pr-str verb))))))
