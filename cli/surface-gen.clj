#!/usr/bin/env bb
;; Render share/help/ (card.txt, topic-<id>.txt, all.txt) from cli/surface.edn.
;; Usage: bb cli/surface-gen.clj [--out <dir>]
;; Command schema: :verb dispatch word · :form typed invocation · :summary ·
;; :topic · :card {:section :form :text} = bare-card row · :aliases · :status
;; :legacy|:unavailable (labeled, last)|:internal (never rendered) · :dispatch
;; :case (default)|:pre-case|:store (passthrough) · :details extra topic lines.

(require '[clojure.edn :as edn]
         '[clojure.string :as str]
         '[babashka.fs :as fs])

(def root (str (fs/parent (fs/parent (fs/absolutize *file*)))))
(def surface (edn/read-string (slurp (str root "/cli/surface.edn"))))

(def out-dir
  (or (second (drop-while #(not= "--out" %) *command-line-args*))
      (str root "/share/help")))

;; -- validation: fail loud at generation time, not render-read time ----------
(let [topic-ids (set (map :id (:topics surface)))
      section-ids (set (map :id (:card-sections surface)))
      cmds (:commands surface)
      verbs (map :verb cmds)]
  (doseq [c cmds]
    (when-not (topic-ids (:topic c))
      (throw (ex-info (str "unknown :topic for verb " (:verb c)) c)))
    (when-let [card (:card c)]
      (when-not (section-ids (:section card))
        (throw (ex-info (str "unknown :card :section for verb " (:verb c)) c))))
    (when-not (contains? #{nil :legacy :unavailable :internal} (:status c))
      (throw (ex-info (str "unknown :status for verb " (:verb c)) c)))
    (when-not (contains? #{nil :case :pre-case :store} (:dispatch c))
      (throw (ex-info (str "unknown :dispatch for verb " (:verb c)) c))))
  (when-not (apply distinct? verbs)
    (throw (ex-info "duplicate :verb entries" {:dups (for [[v n] (frequencies verbs) :when (> n 1)] v)}))))

;; -- rendering ---------------------------------------------------------------
(def col-cap 44) ; forms longer than this get their own line, summary below

(defn pad [s w] (apply str s (repeat (max 0 (- w (count s))) " ")))

(defn rows->lines
  "Two-column rows at `indent`; details/overflow align to the summary column."
  [indent rows]
  (let [width (min col-cap (apply max 1 (map #(count (:form %)) rows)))
        col (+ (count indent) width 2)]
    (mapcat (fn [{:keys [form text details]}]
              (concat
               (if (> (count form) width)
                 [(str indent form)
                  (str (pad "" col) text)]
                 [(str indent (pad form width) "  " text)])
               (map #(str (pad "" col) %) details)))
            rows)))

(defn summary+status [{:keys [summary status aliases]}]
  (str (case status :legacy "legacy — " :unavailable "unavailable — " "")
       summary
       (when (seq aliases)
         (str "  (alias" (when (next aliases) "es") ": " (str/join ", " aliases) ")"))))

(defn topic-rows
  "Renderable rows for one topic: :ok first (registry order), then labeled
  legacy/unavailable; :internal never renders."
  [topic-id]
  (let [cmds (filter #(and (= topic-id (:topic %)) (not= :internal (:status %)))
                     (:commands surface))
        [compat ok] ((juxt filter remove) :status cmds)]
    (for [c (concat ok compat)]
      {:form (:form c) :text (summary+status c) :details (:details c)})))

(defn topic-body [{:keys [id]}]
  (let [lines (rows->lines "  " (topic-rows id))]
    ;; The store topic ends with the engine-passthrough note: Store verbs
    ;; reachable through bin/north's `*` arm have no case arm to register.
    (if (= id :store)
      (concat lines ["" (str "  " (:passthrough surface))])
      lines)))

(defn topic-page [{:keys [id subtitle] :as topic}]
  (concat [(str "north help " (name id) " — " subtitle) ""]
          (topic-body topic)
          ["" (str "  north help --all — the complete reference · bare `north` — the daily card")]))

(def topics-line (str/join " · " (map (comp name :id) (:topics surface))))

(defn card-page []
  (let [by-section (->> (:commands surface)
                        (filter :card)
                        (map-indexed (fn [i c] (assoc c ::idx i)))
                        (sort-by (juxt #(get-in % [:card :rank] 100) ::idx))
                        (group-by (comp :section :card)))
        more-rows [{:form "north help <topic>" :text topics-line}
                   {:form "north help --all" :text "the complete reference"}
                   {:form "north <verb> --help" :text "most verbs print their own usage"}]
        all-rows (concat (mapcat by-section (map :id (:card-sections surface)))
                         [nil])
        width (apply max (map #(count (:form %))
                              (concat (map :card (remove nil? all-rows)) more-rows)))
        row-lines (fn [rows] (rows->lines "    " (map #(assoc % :form (pad (:form %) width)) rows)))]
    (concat
     [(:product surface) ""]
     (mapcat (fn [{:keys [id title]}]
               (concat [(str "  " title)]
                       (row-lines (map :card (by-section id)))
                       [""]))
             (:card-sections surface))
     [(str "  MORE")]
     (row-lines more-rows))))

(defn all-page []
  (concat
   [(str (:product surface) " — the complete reference")
    "(`north help <topic>` is one section; bare `north` is the daily card)"]
   (mapcat (fn [{:keys [title subtitle] :as topic}]
             (concat ["" (str title " — " subtitle)] (topic-body topic)))
           (:topics surface))))

;; -- write, pruning topic files whose topic no longer exists -----------------
(fs/create-dirs out-dir)
(let [want (into {"card.txt" (card-page) "all.txt" (all-page)}
                 (for [t (:topics surface)]
                   [(str "topic-" (name (:id t)) ".txt") (topic-page t)]))]
  (doseq [f (fs/glob out-dir "topic-*.txt")
          :when (not (contains? want (fs/file-name f)))]
    (fs/delete f))
  (doseq [[file lines] want]
    (spit (str out-dir "/" file) (str (str/join "\n" lines) "\n")))
  (println (str "surface-gen: wrote " (count want) " files -> " out-dir)))
