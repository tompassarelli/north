(ns north.agent-catalog
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.java.shell :as shell]
            [clojure.string :as str]))

(def catalog-schema "north.agent-catalog/v1")
(def activation-schema "north.agent-activation/v1")
(def permission-schema "north.agent-permissions/v1")
(def initialization-schema "north.agent-initialization/v1")
(def initialization-receipt-schema "north.agent-initialization-receipt/v1")
(def initialization-transaction-schema "north.agent-initialization-transaction/v1")

(def ^:private unit-id-pattern #"[a-z0-9][a-z0-9-]*")
(def ^:private kinds #{"skill" "hook" "module"})
(def ^:private distribution-types
  #{"skill" "instructions" "hook" "agentTemplates" "providerAdapter"
    "projectPackage"})
(def ^:private distribution-targets
  #{"shared" "codex" "code" "north" "bridge" "firn" "claude"})
(def ^:private permission-pattern
  #"(?:on|off)")
(def ^:private unit-fields
  #{"id" "kind" "title" "triggerDescription" "category" "owner"
    "members" "supports" "distributions"})
(def ^:private source-root
  (some-> *file* io/file .getCanonicalFile .getParentFile .getParentFile str))
(defonce ^:private publication-lock (Object.))
(defonce ^:private revision-cache (atom {}))
(def ^:dynamic *codex-publication-stage!* (fn [_stage _id] nil))

(defn- fail [message data]
  (throw (ex-info message data)))

(defn north-root []
  (or (System/getenv "NORTH_HOME")
      source-root))

(defn catalog-path []
  (or (System/getenv "NORTH_AGENT_CATALOG")
      (str (north-root) "/agent-catalog/catalog.json")))

(defn agents-root []
  (or (not-empty (System/getenv "NORTH_AGENT_STATE_ROOT"))
      (str (System/getenv "HOME") "/.local/state/north/agents")))

(defn- configured-repo-roots []
  (if-let [raw (not-empty (System/getenv "NORTH_REPO_ROOTS"))]
    (let [parsed (json/parse-string raw)]
      (when-not (map? parsed)
        (fail "NORTH_REPO_ROOTS must be a JSON object" {}))
      parsed)
    {}))

(defn repo-root [repo]
  (or (get (configured-repo-roots) repo)
      (when (= repo "north") (north-root))
      (str (System/getenv "HOME") "/code/" repo "/main")))

(defn- relative-owner-path! [owner context]
  (when-not (and (map? owner)
                 (= #{"repo" "path"} (set (keys owner)))
                 (re-matches unit-id-pattern (or (get owner "repo") ""))
                 (string? (get owner "path"))
                 (not (str/blank? (get owner "path"))))
    (fail (str context " has an invalid owner") {:owner owner}))
  (let [path (.normalize (.toPath (io/file (get owner "path"))))]
    (when (or (.isAbsolute path)
              (zero? (.getNameCount path))
              (= ".." (str (.getName path 0))))
      (fail (str context " owner escapes its repository") {:owner owner})))
  owner)

(defn- relative-projection-path! [path context]
  (when-not (and (string? path) (not (str/blank? path)))
    (fail (str context " has an invalid projection path") {:path path}))
  (let [parsed (.normalize (.toPath (io/file path)))]
    (when (or (.isAbsolute parsed) (zero? (.getNameCount parsed))
              (= ".." (str (.getName parsed 0))))
      (fail (str context " projection path escapes its root") {:path path})))
  path)

(defn owner-path
  ([owner] (owner-path owner "owner"))
  ([owner context]
   (relative-owner-path! owner context)
   (let [root (.getCanonicalFile (io/file (repo-root (get owner "repo"))))
         source (.getCanonicalFile (io/file root (get owner "path")))
         root-path (.toPath root)
         source-path (.toPath source)]
     (when-not (.startsWith source-path root-path)
       (fail (str context " owner resolves outside its repository")
             {:owner owner :root (str root) :resolved (str source)}))
     (when-not (.exists source)
       (fail (str context " owner source does not exist")
             {:owner owner :resolved (str source)}))
     source)))

(defn- canonical-value [value]
  (cond
    (map? value) (into (sorted-map)
                       (map (fn [[key item]] [(str key) (canonical-value item)]))
                       value)
    (vector? value) (mapv canonical-value value)
    (sequential? value) (mapv canonical-value value)
    :else value))

(defn- canonical-json [value]
  (json/generate-string (canonical-value value)))

(defn- sha256 [text]
  (let [digest (java.security.MessageDigest/getInstance "SHA-256")]
    (.update digest (.getBytes text java.nio.charset.StandardCharsets/UTF_8))
    (str "sha256:"
         (apply str (map #(format "%02x" (bit-and 0xff %)) (.digest digest))))))

(defn- project-target? [value]
  (boolean (and (string? value)
                (re-matches #"project:[a-z0-9][a-z0-9-]*" value))))

(defn- valid-target? [target]
  (or (distribution-targets target) (project-target? target)))

(defn- unit-targets [unit]
  (mapcat #(get % "targets" []) (get unit "distributions" [])))

(defn- broadly-distributed? [unit]
  (boolean (some distribution-targets (unit-targets unit))))

(defn- project-only? [unit]
  (let [targets (unit-targets unit)]
    (boolean (and (seq targets) (every? project-target? targets)))))

(defn- posix-mode [path]
  (try
    (->> (java.nio.file.Files/getPosixFilePermissions
          path (make-array java.nio.file.LinkOption 0))
         (map str) sort (str/join ","))
    (catch UnsupportedOperationException _ "portable")))

(defn- source-entries [source]
  (let [root (.toPath source)
        real-root (.toRealPath root (make-array java.nio.file.LinkOption 0))]
    (with-open [stream (java.nio.file.Files/walk
                        root
                        (into-array java.nio.file.FileVisitOption
                                    [java.nio.file.FileVisitOption/FOLLOW_LINKS]))]
      (->> (iterator-seq (.iterator stream))
           (map (fn [path]
                  (let [real (.toRealPath path (make-array java.nio.file.LinkOption 0))]
                    (when-not (.startsWith real real-root)
                      (fail "owner payload contains an escaping symlink"
                            {:source (str source) :path (str path) :resolved (str real)}))
                    path)))
           (sort-by #(str (.relativize root %)))
           vec))))

(defn- digest-source [source]
  (let [digest (java.security.MessageDigest/getInstance "SHA-256")
        root (.toPath source)
        update! (fn [value]
                  (.update digest (.getBytes (str value "\u0000")
                                             java.nio.charset.StandardCharsets/UTF_8)))]
    (doseq [path (source-entries source)]
      (let [relative (str (.relativize root path))
            directory? (java.nio.file.Files/isDirectory
                        path (make-array java.nio.file.LinkOption 0))]
        (update! relative)
        (update! (if directory? "directory" "file"))
        (update! (posix-mode path))
        (when-not directory?
          (.update digest (java.nio.file.Files/readAllBytes path)))))
    (str "sha256:"
         (apply str (map #(format "%02x" (bit-and 0xff %)) (.digest digest))))))

(defn- owner-revision [owner]
  (let [repo (get owner "repo")]
    (or (get @revision-cache repo)
        (let [{:keys [exit out err]} (shell/sh "git" "-C" (repo-root repo)
                                               "rev-parse" "HEAD")]
          (when-not (zero? exit)
            (fail (str "cannot resolve owner revision for " repo ": " (str/trim err))
                  {:repo repo}))
          (let [revision (str/trim out)]
            (swap! revision-cache assoc repo revision)
            revision)))))

(defn- provenance [owner context]
  (let [source (owner-path owner context)]
    {"owner" owner
     "revision" (owner-revision owner)
     "contentDigest" (digest-source source)}))

(defn- scalar-yaml [value]
  (let [value (str/trim value)]
    (if (and (<= 2 (count value))
             (= (first value) (last value))
             (#{\" \'} (first value)))
      (subs value 1 (dec (count value)))
      value)))

(defn- skill-frontmatter [path]
  (let [lines (vec (str/split-lines (slurp path)))
        end (first (keep-indexed
                    (fn [index line]
                      (when (and (pos? index) (= "---" line)) index))
                    lines))]
    (when-not (and (= "---" (first lines)) end)
      (fail (str "skill has invalid YAML frontmatter: " path) {:path (str path)}))
    (let [frontmatter (subvec lines 1 end)
          fields
          (loop [index 0 result {}]
            (if (>= index (count frontmatter))
              result
              (let [line (nth frontmatter index)]
                (if-let [[_ key raw] (and (not (re-find #"^\s" line))
                                          (re-matches #"([A-Za-z][A-Za-z0-9_-]*):\s*(.*)" line))]
                  (if (#{">" ">-" "|" "|-"} raw)
                    (let [body (take-while #(or (str/blank? %) (re-find #"^\s" %))
                                           (drop (inc index) frontmatter))
                          text (->> body (map str/trim) (remove str/blank?)
                                    ((if (str/starts-with? raw ">")
                                       #(str/join " " %)
                                       #(str/join "\n" %))))]
                      (recur (+ index 1 (count body)) (assoc result key text)))
                    (recur (inc index) (assoc result key (scalar-yaml raw))))
                  (recur (inc index) result)))))]
      fields)))

(defn- human-title [id]
  (->> (str/split id #"-")
       (map str/capitalize)
       (str/join " ")))

(defn- distribution-owner [unit distribution]
  (or (get distribution "owner")
      (let [owner (get unit "owner")]
        (if (and (= "skill" (get distribution "type"))
                 (str/ends-with? (get owner "path") "/SKILL.md"))
          (assoc owner "path" (str/replace (get owner "path") #"/SKILL\.md$" ""))
          owner))))

(defn- validate-distribution! [unit index distribution]
  (let [context (str "unit " (get unit "id") " distribution " index)
        type (get distribution "type")
        targets (get distribution "targets")
        owner (distribution-owner unit distribution)]
    (when-not (distribution-types type)
      (fail (str context " has an invalid type") {:type type}))
    (when-not (and (vector? targets) (seq targets)
                   (= (count targets) (count (distinct targets)))
                   (every? valid-target? targets))
      (fail (str context " has invalid or duplicate targets") {:targets targets}))
    (owner-path owner context)
    (-> distribution
        (assoc "owner" owner
               "adapterId" (or (get distribution "adapterId")
                               (let [basename (.getName (io/file (get owner "path")))]
                                 (if (= basename "openai.yaml")
                                   (str (get unit "id") "-" basename)
                                   basename)))
               "provenance" (provenance owner context))
        (dissoc "source"))))

(defn- validate-unit-shape! [unit]
  (let [id (get unit "id")
        kind (get unit "kind")
        unknown-fields (sort (remove unit-fields (keys unit)))]
    (when-not (re-matches unit-id-pattern (or id ""))
      (fail "catalog has an invalid unit id" {:id id}))
    (when-not (kinds kind)
      (fail (str "unit " id " has an invalid kind") {:kind kind}))
    (when (seq unknown-fields)
      (fail (str "unit " id " has unsupported fields: "
                 (str/join ", " unknown-fields))
            {:id id :fields unknown-fields}))
    (owner-path (get unit "owner") (str "unit " id))
    (when-not (and (vector? (get unit "distributions"))
                   (seq (get unit "distributions")))
      (fail (str "unit " id " has no distributions") {:id id}))
    (when (and (= kind "module")
               (or (str/blank? (get unit "title" ""))
                   (str/blank? (get unit "triggerDescription" ""))))
      (fail (str "module " id " must declare title and triggerDescription") {:id id}))
    unit))

(defn- exact-module-cycles! [units by-id]
  (let [color (atom {})]
    (letfn [(visit [id stack]
              (when (= "module" (get-in by-id [id "kind"]))
                (case (get @color id)
                  :grey (let [start (.indexOf stack id)
                              cycle (conj (subvec stack start) id)]
                          (fail (str "catalog module cycle: " (str/join " -> " cycle))
                                {:cycle cycle}))
                  :black nil
                  (do
                    (swap! color assoc id :grey)
                    (doseq [member (get-in by-id [id "members"])]
                      (when (= "module" (get-in by-id [member "kind"]))
                        (visit member (conj stack id))))
                    (swap! color assoc id :black)))))]
      (doseq [unit units :when (= "module" (get unit "kind"))]
        (visit (get unit "id") [])))))

(defn load-catalog []
  (reset! revision-cache {})
  (let [path (catalog-path)
        catalog (try (json/parse-string (slurp path))
                     (catch Exception error
                       (fail (str "cannot read agent catalog: " (.getMessage error))
                             {:path path})))
        units (get catalog "units")
        baselines (get catalog "baselines")
        provider-support (get catalog "providerSupport")]
    (when-not (= catalog-schema (get catalog "schema"))
      (fail "unsupported agent catalog schema" {:schema (get catalog "schema")}))
    (when-not (and (vector? units) (seq units))
      (fail "agent catalog units must be a non-empty array" {}))
    (when-not (and (vector? baselines) (seq baselines))
      (fail "agent catalog baselines must be a non-empty array" {}))
    (when-not (and (vector? provider-support) (seq provider-support))
      (fail "agent catalog providerSupport must be a non-empty array" {}))
    (doseq [[index baseline] (map-indexed vector baselines)]
      (let [context (str "baseline " index)
            targets (get baseline "targets")]
        (when-not (and (re-matches unit-id-pattern (or (get baseline "id") ""))
                       (vector? targets) (seq targets)
                       (= (count targets) (count (distinct targets)))
                       (every? #{"shared" "codex" "code"} targets))
          (fail (str context " is invalid") {:baseline baseline}))
        (owner-path (get baseline "owner") context)))
    (doseq [[index support] (map-indexed vector provider-support)]
      (let [context (str "provider support " index)]
        (when-not (re-matches unit-id-pattern (or (get support "id") ""))
          (fail (str context " has an invalid id") {:support support}))
        (relative-projection-path! (get support "path") context)
        (owner-path (get support "owner") context)))
    (doseq [unit units] (validate-unit-shape! unit))
    (let [ids (mapv #(get % "id") units)
          duplicates (->> ids frequencies (keep (fn [[id n]] (when (> n 1) id))) sort vec)]
      (when (seq duplicates)
        (fail (str "duplicate catalog unit ids: " (str/join ", " duplicates))
              {:ids duplicates})))
    (let [by-id (into {} (map (juxt #(get % "id") identity)) units)
          roots (get catalog "rootOrder")]
      (when-not (vector? roots)
        (fail "catalog rootOrder must be an array" {}))
      (when-not (= (count roots) (count (distinct roots)))
        (fail "catalog rootOrder contains duplicates" {:rootOrder roots}))
      (doseq [root roots]
        (when-not (contains? by-id root)
          (fail (str "catalog rootOrder names unknown unit " root) {:id root})))
      (doseq [unit units]
        (let [id (get unit "id")
              kind (get unit "kind")
              members (get unit "members" [])
              supports (get unit "supports" [])]
          (when-not (and (vector? members) (= (count members) (count (distinct members))))
            (fail (str "unit " id " has duplicate or invalid members") {:members members}))
          (when (and (not= kind "module") (seq members))
            (fail (str "non-module unit " id " declares members") {:members members}))
          (doseq [member members]
            (when-not (contains? by-id member)
              (fail (str "module " id " names unknown member " member) {:id id :member member}))
            (when (and (broadly-distributed? unit)
                       (project-only? (get by-id member)))
              (fail (str "broadly distributed module " id
                         " contains project-only member " member)
                    {:id id :member member})))
          (when-not (and (vector? supports) (= (count supports) (count (distinct supports))))
            (fail (str "unit " id " has duplicate or invalid supports") {:supports supports}))
          (when (and (not= kind "hook") (seq supports))
            (fail (str "non-hook unit " id " declares supports") {:supports supports}))
          (doseq [supported supports]
            (when-not (#{"skill" "module"} (get-in by-id [supported "kind"]))
              (fail (str "hook " id " supports unknown or invalid unit " supported)
                    {:id id :supports supported}))
            (when (and (#{"skill" "module"} (get-in by-id [supported "kind"]))
                       (project-only? unit)
                       (broadly-distributed? (get by-id supported)))
              (fail (str "broadly distributed unit " supported
                         " cannot depend on project-only hook " id)
                    {:id id :supports supported})))))
      (exact-module-cycles! units by-id)
      (let [enriched
            (mapv
             (fn [unit]
               (let [owner-file (owner-path (get unit "owner") (str "unit " (get unit "id")))
                     metadata (when (= "skill" (get unit "kind"))
                                (skill-frontmatter owner-file))
                     id (get unit "id")
                     declared-name (get metadata "name")
                     declared-category (not-empty (get metadata "category"))
                     title (or (not-empty (get unit "title")) (human-title id))
                     trigger (or (not-empty (get unit "triggerDescription"))
                                 (not-empty (get metadata "description")))]
                 (when (and metadata (not= id declared-name))
                   (fail (str "skill " id " source declares name " (pr-str declared-name))
                         {:id id :declaredName declared-name}))
                 (when (and declared-category
                            (not= (get unit "category") declared-category))
                   (fail (str "skill " id " source declares category "
                              (pr-str declared-category))
                         {:id id
                          :catalogCategory (get unit "category")
                          :declaredCategory declared-category}))
                 (when (str/blank? trigger)
                   (fail (str "unit " id " has no triggerDescription") {:id id}))
                 (-> unit
                     (assoc "title" title
                            "triggerDescription" trigger
                            "ownerProvenance" (provenance (get unit "owner")
                                                          (str "unit " id))
                            "members" (get unit "members" [])
                            "supports" (get unit "supports" [])
                            "distributions"
                            (mapv #(validate-distribution! unit %1 %2)
                                  (range) (get unit "distributions"))))))
             units)]
        {:path path
         :catalog catalog
         :digest (sha256 (canonical-json catalog))
         :baselines (mapv (fn [baseline]
                            (assoc baseline "provenance"
                                   (provenance (get baseline "owner")
                                               (str "baseline " (get baseline "id")))))
                          baselines)
         :provider-support
         (mapv (fn [support]
                 (assoc support "provenance"
                        (provenance (get support "owner")
                                    (str "provider support " (get support "id")))))
               provider-support)
         :units enriched
         :by-id (into {} (map (juxt #(get % "id") identity)) enriched)
         :root-order roots}))))

(defn- permission-live? [permission]
  (= permission "on"))

(defn default-permissions [catalog]
  (into (sorted-map)
        (map (fn [unit] [(get unit "id") "off"]))
        (:units catalog)))

(defn current-activation []
  (let [path (io/file (agents-root) "current/activation.json")]
    (when (.isFile path)
      (let [activation (json/parse-string (slurp path))]
        (when-not (= activation-schema (get activation "schema"))
          (fail "current agent activation has an unsupported schema"
                {:path (str path) :schema (get activation "schema")}))
        activation))))

(defn current-permissions [catalog]
  (let [known (set (map #(get % "id") (:units catalog)))
        previous (get (current-activation) "permissions" {})
        surviving (select-keys previous known)]
    (doseq [[id permission] surviving]
      (when-not (and (string? permission)
                     (re-matches permission-pattern permission))
        (fail (str "current activation contains an invalid permission for " id)
              {:id id :permission permission})))
    (merge (default-permissions catalog) surviving)))

(defn- exact-keys! [value required optional context]
  (when-not (map? value)
    (fail (str context " must be an object") {:value value}))
  (let [actual (set (keys value))
        missing (sort (remove actual required))
        unsupported (sort (remove (into required optional) actual))]
    (when (seq missing)
      (fail (str context " omits fields: " (str/join ", " missing))
            {:fields missing}))
    (when (seq unsupported)
      (fail (str context " has unsupported fields: "
                 (str/join ", " unsupported))
            {:fields unsupported})))
  value)

(defn- nonblank-string! [value context]
  (when-not (and (string? value) (not (str/blank? value)))
    (fail (str context " must be a non-empty string") {:value value}))
  value)

(defn- lexical-owner-target [owner context]
  (relative-owner-path! owner context)
  (let [root (.normalize (.toAbsolutePath (.toPath (io/file (repo-root (get owner "repo"))))))
        target (.normalize (.toAbsolutePath
                            (.toPath (io/file (str root) (get owner "path")))))]
    (when-not (.startsWith target root)
      (fail (str context " resolves outside its repository")
            {:owner owner :root (str root) :resolved (str target)}))
    target))

(defn load-initialization [catalog path]
  (let [document
        (try
          (json/parse-string (slurp path))
          (catch Exception error
            (fail (str "cannot read agent initialization: " (.getMessage error))
                  {:path path})))]
    (exact-keys! document
                 #{"schema" "id" "permissionAuthorities" "permissionResolutions"
                   "skillLinks" "retiredPermissionObservations"}
                 #{} "agent initialization")
    (when-not (= initialization-schema (get document "schema"))
      (fail "unsupported agent initialization schema"
            {:schema (get document "schema")}))
    (when-not (re-matches unit-id-pattern (or (get document "id") ""))
      (fail "agent initialization has an invalid id" {:id (get document "id")}))
    (let [authorities (get document "permissionAuthorities")
          resolutions (get document "permissionResolutions")
          links (get document "skillLinks")
          retired (get document "retiredPermissionObservations")
          known (:by-id catalog)]
      (when-not (and (vector? authorities) (seq authorities))
        (fail "agent initialization permissionAuthorities must be a non-empty array" {}))
      (when-not (map? resolutions)
        (fail "agent initialization permissionResolutions must be an object" {}))
      (when-not (vector? links)
        (fail "agent initialization skillLinks must be an array" {}))
      (when-not (vector? retired)
        (fail "agent initialization retiredPermissionObservations must be an array" {}))
      (let [authority-ids (mapv #(get % "id") authorities)
            duplicates (->> authority-ids frequencies
                            (keep (fn [[id n]] (when (> n 1) id))) sort vec)
            observed
            (reduce
             (fn [result [index authority]]
               (let [context (str "permission authority " index)]
                 (exact-keys! authority #{"id" "source" "permissions"}
                              #{"sourceIds"} context)
                 (when-not (re-matches unit-id-pattern (or (get authority "id") ""))
                   (fail (str context " has an invalid id") {:authority authority}))
                 (nonblank-string! (get authority "source") (str context " source"))
                 (let [permissions (get authority "permissions")
                       source-ids (get authority "sourceIds" {})]
                   (when-not (and (map? permissions) (seq permissions))
                     (fail (str context " permissions must be a non-empty object") {}))
                   (when-not (map? source-ids)
                     (fail (str context " sourceIds must be an object") {}))
                   (let [orphan-source-ids (sort (remove (set (keys permissions))
                                                         (keys source-ids)))]
                     (when (seq orphan-source-ids)
                       (fail (str context " sourceIds name unobserved units: "
                                  (str/join ", " orphan-source-ids))
                             {:ids orphan-source-ids})))
                   (doseq [[unit-id source-id] source-ids]
                     (when-not (re-matches unit-id-pattern (or source-id ""))
                       (fail (str context " has an invalid source UnitId for " unit-id)
                             {:id unit-id :sourceId source-id})))
                   (reduce-kv
                    (fn [rows unit-id permission]
                      (when-not (contains? known unit-id)
                        (fail (str context " names unknown unit " unit-id)
                              {:id unit-id}))
                      (when-not (and (string? permission)
                                     (re-matches permission-pattern permission))
                        (fail (str context " has invalid permission for " unit-id)
                              {:id unit-id :permission permission}))
                      (update rows unit-id (fnil conj [])
                              {:authority (get authority "id")
                               :source-id (get source-ids unit-id unit-id)
                               :permission permission}))
                    result permissions))))
             {}
             (map-indexed vector authorities))]
        (when (seq duplicates)
          (fail (str "duplicate initialization authorities: "
                     (str/join ", " duplicates))
                {:ids duplicates}))
        (let [conflicts
              (into (sorted-map)
                    (keep (fn [[unit-id rows]]
                            (let [values (set (map :permission rows))]
                              (when (> (count values) 1) [unit-id rows]))))
                    observed)
              conflict-ids (set (keys conflicts))
              resolution-ids (set (keys resolutions))
              unresolved (sort (remove resolution-ids conflict-ids))
              unnecessary (sort (remove conflict-ids resolution-ids))]
          (when (seq unresolved)
            (fail (str "conflicting initialization authorities for: "
                       (str/join ", " unresolved))
                  {:ids unresolved :observations (select-keys conflicts unresolved)}))
          (when (seq unnecessary)
            (fail (str "initialization resolutions do not resolve conflicts: "
                       (str/join ", " unnecessary))
                  {:ids unnecessary}))
          (doseq [[unit-id resolution] resolutions]
            (exact-keys! resolution #{"permission" "reason"} #{}
                         (str "permission resolution " unit-id))
            (let [permission (get resolution "permission")
                  observed-values (set (map :permission (get conflicts unit-id)))]
              (when-not (observed-values permission)
                (fail (str "permission resolution " unit-id
                           " does not select an observed value")
                      {:id unit-id :permission permission
                       :observed observed-values})))
            (nonblank-string! (get resolution "reason")
                              (str "permission resolution " unit-id " reason")))
          (let [permissions
                (reduce-kv
                 (fn [result unit-id rows]
                   (assoc result unit-id
                          (or (get-in resolutions [unit-id "permission"])
                              (:permission (first rows)))))
                 (default-permissions catalog) observed)
                link-ids (mapv #(get % "id") links)
                duplicate-links (->> link-ids frequencies
                                     (keep (fn [[id n]] (when (> n 1) id))) sort vec)
                prepared-links
                (mapv
                 (fn [[index link]]
                   (let [context (str "initial skill link " index)
                         id (get link "id")
                         action (get link "action")]
                     (exact-keys! link #{"id" "owner" "action"} #{} context)
                     (when-not (re-matches unit-id-pattern (or id ""))
                       (fail (str context " has an invalid id") {:id id}))
                     (when-not (#{"adopt" "retire"} action)
                       (fail (str context " has an invalid action") {:action action}))
                     (let [unit (get known id)]
                       (if (= action "adopt")
                         (when-not (and (= "skill" (get unit "kind"))
                                        (some #(and (= "skill" (get % "type"))
                                                    (some #{"shared"} (get % "targets")))
                                              (get unit "distributions")))
                           (fail (str context " does not name a shared catalog skill")
                                 {:id id}))
                         (when unit
                           (fail (str context " retires a current catalog unit") {:id id}))))
                     (assoc link "target"
                            (lexical-owner-target (get link "owner") context))))
                 (map-indexed vector links))]
            (when (seq duplicate-links)
              (fail (str "duplicate initial skill links: "
                         (str/join ", " duplicate-links))
                    {:ids duplicate-links}))
            (let [target-groups (group-by #(str (get % "target")) prepared-links)
                  duplicate-targets (->> target-groups
                                         (keep (fn [[target entries]]
                                                 (when (> (count entries) 1) target)))
                                         sort vec)]
              (when (seq duplicate-targets)
                (fail "initial skill ownership targets are ambiguous"
                      {:targets duplicate-targets})))
            (doseq [[index observation] (map-indexed vector retired)]
              (let [context (str "retired permission observation " index)]
                (exact-keys! observation
                             #{"authority" "sourceId" "permission" "reason"}
                             #{} context)
                (nonblank-string! (get observation "authority")
                                  (str context " authority"))
                (when-not (re-matches unit-id-pattern (or (get observation "sourceId") ""))
                  (fail (str context " has an invalid source UnitId")
                        {:sourceId (get observation "sourceId")}))
                (when-not (re-matches permission-pattern
                                      (or (get observation "permission") ""))
                  (fail (str context " has an invalid permission")
                        {:permission (get observation "permission")}))
                (nonblank-string! (get observation "reason")
                                  (str context " reason"))))
            {:document document
             :digest (sha256 (canonical-json document))
             :permissions permissions
             :skill-links (into {} (map (juxt #(get % "id") identity))
                                prepared-links)}))))))

(defn- validate-permissions! [catalog permissions]
  (let [known (set (map #(get % "id") (:units catalog)))
        unknown (sort (remove known (keys permissions)))
        missing (sort (remove (set (keys permissions)) known))]
    (when (seq unknown)
      (fail (str "permissions name unknown units: " (str/join ", " unknown))
            {:ids unknown}))
    (when (seq missing)
      (fail (str "permissions omit units: " (str/join ", " missing)) {:ids missing}))
    (doseq [[id permission] permissions]
      (when-not (and (string? permission) (re-matches permission-pattern permission))
        (fail (str "unit " id " has invalid permission " (pr-str permission))
              {:id id :permission permission})))
    permissions))

(defn compile-activation
  ([catalog] (compile-activation catalog (current-permissions catalog)))
  ([catalog permissions]
   (validate-permissions! catalog permissions)
   (let [units (:units catalog)
         by-id (:by-id catalog)
         live? #(permission-live? (get permissions %))
         supporting
         (reduce
          (fn [index unit]
            (if (= "hook" (get unit "kind"))
              (reduce (fn [result supported]
                        (update result supported (fnil conj []) (get unit "id")))
                      index
                      (get unit "supports"))
              index))
          {}
          units)
         paths (atom {})
         active-order (atom [])]
     (letfn [(record! [id path]
               (when-not (contains? @paths id)
                 (swap! active-order conj id))
               (swap! paths update id
                      (fn [existing]
                        (let [existing (or existing [])]
                          (if (some #(= path %) existing) existing (conj existing path))))))
             (walk! [id parent-path]
               (when (live? id)
                 (let [path (conj parent-path id)
                       unit (get by-id id)]
                   (record! id path)
                   (when (= "module" (get unit "kind"))
                     (doseq [member (get unit "members")]
                       (walk! member path)))
                   (doseq [hook-id (get supporting id)]
                     (walk! hook-id path)))))]
       (doseq [root (:root-order catalog)] (walk! root [])))
     (let [order (vec (concat @active-order
                              (remove (set @active-order) (map #(get % "id") units))))
           resolved
           (mapv
            (fn [id]
              (let [unit (get by-id id)
                    activation-paths (get @paths id [])]
                (-> (select-keys unit ["id" "kind" "title" "triggerDescription"
                                       "owner" "members" "supports" "distributions"
                                       "category" "ownerProvenance"])
                    (assoc "permission" (get permissions id)
                           "active" (boolean (seq activation-paths))
                           "activationPaths" activation-paths))))
            order)
           plan
           (reduce
            (fn [result unit]
              (reduce
               (fn [plan distribution]
                 (if (or (get unit "active")
                         (#{"hook" "providerAdapter"} (get distribution "type")))
                   (reduce
                    (fn [targets target]
                      (update-in targets [(get distribution "type") target]
                                 (fnil conj [])
                                 {"unitId" (get unit "id")
                                  "owner" (get distribution "owner")
                                  "adapterId" (get distribution "adapterId")
                                  "provenance" (get distribution "provenance")}))
                    plan
                    (get distribution "targets"))
                   plan))
               result
               (get unit "distributions")))
            (sorted-map)
            resolved)
           identity-input
           {"schema" activation-schema
            "catalogSchema" catalog-schema
            "catalogDigest" (:digest catalog)
            "baselines" (:baselines catalog)
            "providerSupport" (:provider-support catalog)
            "permissions" (into (sorted-map) permissions)
            "rootOrder" (:root-order catalog)
            "units" resolved
            "projectionPlan" plan}
           generation-id (sha256 (canonical-json identity-input))]
       (assoc identity-input "generationId" generation-id)))))

(defn- delete-tree! [path]
  (when (.exists (io/file path))
    (with-open [stream (java.nio.file.Files/walk (.toPath (io/file path))
                                                 (make-array java.nio.file.FileVisitOption 0))]
      (doseq [entry (reverse (iterator-seq (.iterator stream)))]
        (java.nio.file.Files/deleteIfExists entry)))))

(defn- ensure-root! []
  (let [root (.toPath (io/file (agents-root)))]
    (java.nio.file.Files/createDirectories
     root (make-array java.nio.file.attribute.FileAttribute 0))
    (when (and (java.nio.file.Files/exists (.resolve root "current")
                                           (make-array java.nio.file.LinkOption 0))
               (not (java.nio.file.Files/isSymbolicLink (.resolve root "current"))))
      (fail "refusing to replace unmanaged agent activation current path"
            {:path (str (.resolve root "current"))}))
    root))

(defn- with-publication-lock [f]
  (locking publication-lock
    (let [root (ensure-root!)
          path (.resolve root ".lock")]
      (try
        (java.nio.file.Files/createFile
         path (make-array java.nio.file.attribute.FileAttribute 0))
        (catch java.nio.file.FileAlreadyExistsException _))
      (when (or (java.nio.file.Files/isSymbolicLink path)
                (not (java.nio.file.Files/isRegularFile
                      path
                      (into-array java.nio.file.LinkOption
                                  [java.nio.file.LinkOption/NOFOLLOW_LINKS]))))
        (fail "agent activation lock must be a regular file" {:path (str path)}))
      (with-open [channel
                  (java.nio.channels.FileChannel/open
                   path
                   (into-array java.nio.file.OpenOption
                               [java.nio.file.StandardOpenOption/WRITE
                                java.nio.file.LinkOption/NOFOLLOW_LINKS]))]
        (let [_held (.lock channel)]
          (f))))))

(defn- skill-plan [activation target]
  (get-in activation ["projectionPlan" "skill" target] []))

(defn- set-mode! [source target]
  (try
    (java.nio.file.Files/setPosixFilePermissions
     target (java.nio.file.Files/getPosixFilePermissions
             source (make-array java.nio.file.LinkOption 0)))
    (catch UnsupportedOperationException _))
  target)

(defn- copy-source! [source target]
  (let [source-path (.toPath source)]
    (source-entries source)
    (if (java.nio.file.Files/isDirectory source-path
                                         (make-array java.nio.file.LinkOption 0))
      (do
        (java.nio.file.Files/createDirectories
         target (make-array java.nio.file.attribute.FileAttribute 0))
        (doseq [path (source-entries source)
                :let [relative (.relativize source-path path)
                      destination (.resolve target relative)]
                :when (pos? (.getNameCount relative))]
          (if (java.nio.file.Files/isDirectory path
                                               (make-array java.nio.file.LinkOption 0))
            (java.nio.file.Files/createDirectories
             destination (make-array java.nio.file.attribute.FileAttribute 0))
            (do
              (java.nio.file.Files/createDirectories
               (.getParent destination)
               (make-array java.nio.file.attribute.FileAttribute 0))
              (java.nio.file.Files/copy
               path destination
               (into-array java.nio.file.CopyOption
                           [java.nio.file.StandardCopyOption/REPLACE_EXISTING]))))
          (set-mode! path destination))
        (set-mode! source-path target))
      (do
        (java.nio.file.Files/createDirectories
         (.getParent target) (make-array java.nio.file.attribute.FileAttribute 0))
        (java.nio.file.Files/copy
         source-path target
         (into-array java.nio.file.CopyOption
                     [java.nio.file.StandardCopyOption/REPLACE_EXISTING]))
        (set-mode! source-path target)))))

(defn- write-instructions! [temporary activation target]
  (let [baseline (for [entry (get activation "baselines")
                       :when (some #{target} (get entry "targets"))]
                   {"unitId" (get entry "id") "owner" (get entry "owner")})
        active (get-in activation ["projectionPlan" "instructions" target] [])
        entries (concat baseline active)
        destination (.resolve temporary (str "instructions/" target "/AGENTS.md"))]
    (java.nio.file.Files/createDirectories
     (.getParent destination) (make-array java.nio.file.attribute.FileAttribute 0))
    (spit (str destination)
          (str/join
           "\n\n"
           (for [entry entries
                 :let [owner (get entry "owner")
                       source (owner-path owner (str "instructions " (get entry "unitId")))]]
             (str "<!-- " (get owner "repo") ":" (get owner "path") " -->\n"
                  (slurp source)))))))

(defn- instruction-targets [activation]
  (->> (concat (mapcat #(get % "targets") (get activation "baselines"))
               (keys (get-in activation ["projectionPlan" "instructions"])))
       distinct
       sort))

(defn- stage-skills! [temporary activation]
  (let [directory (.resolve temporary "skills/shared")]
    (java.nio.file.Files/createDirectories
     directory (make-array java.nio.file.attribute.FileAttribute 0))
    (doseq [entry (skill-plan activation "shared")]
      (let [id (get entry "unitId")
            source (owner-path (get entry "owner") (str "skill projection " id))]
        (copy-source! source (.resolve directory id))))))

(defn- stage-projects! [temporary activation]
  (doseq [[type targets] (get activation "projectionPlan")
          [target entries] targets
          :when (project-target? target)
          entry entries]
    (let [repo (subs target (count "project:"))
          id (get entry "unitId")
          source (owner-path (get entry "owner") (str "project projection " id))]
      (copy-source! source (.resolve temporary (str "projects/" repo "/" type "/" id))))))

(defn- stage-agent-templates! [temporary activation]
  (doseq [[target entries] (get-in activation ["projectionPlan" "agentTemplates"])
          entry entries]
    (let [id (get entry "unitId")
          source (owner-path (get entry "owner") (str "agent templates " id))]
      (copy-source! source
                    (.resolve temporary (str "agent-templates/" target "/" id))))))

(defn- stage-provider-hooks! [temporary activation]
  (let [directory (.resolve temporary "provider-hooks")
        entries (for [type ["hook" "providerAdapter"]
                      [_target items] (get-in activation ["projectionPlan" type])
                      entry items]
                  entry)
        unique-entries (vec (distinct entries))
        collisions (->> unique-entries (group-by #(get % "adapterId"))
                        (keep (fn [[id values]] (when (> (count values) 1) id))) sort vec)
        distinct-entries (sort-by #(get % "adapterId") unique-entries)]
    (when (seq collisions)
      (fail (str "provider adapter ids collide: " (str/join ", " collisions))
            {:adapterIds collisions}))
    (java.nio.file.Files/createDirectories
     directory (make-array java.nio.file.attribute.FileAttribute 0))
    (doseq [support (get activation "providerSupport")]
      (let [source (owner-path (get support "owner")
                               (str "provider support " (get support "id")))]
        (copy-source! source (.resolve directory (get support "path")))))
    (doseq [entry distinct-entries]
      (let [id (get entry "unitId")
            source (owner-path (get entry "owner") (str "provider adapter " id))]
        (copy-source! source (.resolve directory (get entry "adapterId")))))))

(defn- stage-generation! [activation]
  (let [root (ensure-root!)
        suffix (subs (get activation "generationId") (count "sha256:"))
        generation (.resolve root (str "gen-" suffix))
        temporary (.resolve root (str ".gen-" suffix ".tmp-" (java.util.UUID/randomUUID)))]
    (if (java.nio.file.Files/isDirectory generation (make-array java.nio.file.LinkOption 0))
      generation
      (try
        (java.nio.file.Files/createDirectory
         temporary (make-array java.nio.file.attribute.FileAttribute 0))
        (stage-skills! temporary activation)
        (doseq [target (instruction-targets activation)]
          (write-instructions! temporary activation target))
        (stage-projects! temporary activation)
        (stage-agent-templates! temporary activation)
        (stage-provider-hooks! temporary activation)
        (spit (str (.resolve temporary "activation.json"))
              (str (json/generate-string activation {:pretty true}) "\n"))
        (try
          (java.nio.file.Files/move
           temporary generation
           (into-array java.nio.file.CopyOption
                       [java.nio.file.StandardCopyOption/ATOMIC_MOVE]))
          (catch java.nio.file.FileAlreadyExistsException _
            (delete-tree! temporary)))
        generation
        (catch Throwable error
          (delete-tree! temporary)
          (throw error))))))

(defn- resolved-link-target [path]
  (when (java.nio.file.Files/isSymbolicLink path)
    (-> (.resolve (.getParent path)
                  (java.nio.file.Files/readSymbolicLink path))
        .toAbsolutePath
        .normalize)))

(defn- raw-link-target [path]
  (when (java.nio.file.Files/isSymbolicLink path)
    (java.nio.file.Files/readSymbolicLink path)))

(defn codex-skills-dir []
  (.toPath (io/file (str (System/getenv "HOME") "/.codex/skills"))))

(defn- exists-no-follow? [path]
  (java.nio.file.Files/exists
   path (into-array java.nio.file.LinkOption
                    [java.nio.file.LinkOption/NOFOLLOW_LINKS])))

(defn- read-managed-skill-ids [manifest]
  (if-not (exists-no-follow? manifest)
    nil
    (do
      (when-not (java.nio.file.Files/isRegularFile
                 manifest (into-array java.nio.file.LinkOption
                                      [java.nio.file.LinkOption/NOFOLLOW_LINKS]))
        (fail "Codex managed-skill manifest must be a regular file"
              {:path (str manifest)}))
      (let [document (try
                       (json/parse-string (slurp (str manifest)))
                       (catch Exception error
                         (fail (str "cannot read Codex managed-skill manifest: "
                                    (.getMessage error))
                               {:path (str manifest)})))
            ids (get document "ids")]
        (when-not (and (= "north.codex-managed-skills/v1" (get document "schema"))
                       (= #{"schema" "ids"} (set (keys document)))
                       (vector? ids)
                       (= (count ids) (count (distinct ids)))
                       (every? #(and (string? %) (re-matches unit-id-pattern %)) ids))
          (fail "Codex managed-skill manifest is invalid" {:path (str manifest)}))
        (set ids)))))

(defn- assert-link-target! [path target message id]
  (when-not (= (resolved-link-target path) target)
    (fail (str message ": " id)
          {:id id :path (str path) :expected (str target)
           :actual (some-> (resolved-link-target path) str)})))

(defn- assert-literal-link-target! [path target message id]
  (when-not (= (raw-link-target path) target)
    (fail (str message ": " id)
          {:id id :path (str path) :expected (str target)
           :actual (some-> (raw-link-target path) str)})))

(defn- atomic-link! [path target replace?]
  (let [temporary (.resolve (.getParent path)
                            (str ".north-" (.getFileName path) "-"
                                 (java.util.UUID/randomUUID) ".tmp"))]
    (try
      (java.nio.file.Files/createSymbolicLink
       temporary target (make-array java.nio.file.attribute.FileAttribute 0))
      (java.nio.file.Files/move
       temporary path
       (into-array java.nio.file.CopyOption
                   (cond-> [java.nio.file.StandardCopyOption/ATOMIC_MOVE]
                     replace? (conj java.nio.file.StandardCopyOption/REPLACE_EXISTING))))
      (catch Throwable error
        (java.nio.file.Files/deleteIfExists temporary)
        (throw error)))))

(defn- managed-manifest-content [active]
  (str (json/generate-string
        {"schema" "north.codex-managed-skills/v1"
         "ids" (vec (sort active))}
        {:pretty true})
       "\n"))

(defn- initialization-plan [activation initialization root directory]
  (let [active (set (map #(get % "unitId") (skill-plan activation "shared")))
        known-skills (set (for [unit (get activation "units")
                                :when (= "skill" (get unit "kind"))]
                            (get unit "id")))
        links (:skill-links initialization)
        ids (sort (into known-skills (keys links)))
        current (.toAbsolutePath (.resolve root "current/skills/shared"))]
    (mapv
     (fn [id]
       (let [entry (get links id)
             action (get entry "action")
             operation (cond
                         (= "retire" action) "remove"
                         (and (active id) (= "adopt" action)) "replace"
                         (active id) "create"
                         (= "adopt" action) "remove"
                         :else "absent")
             old-target (when (#{"replace" "remove"} operation)
                          (get entry "target"))
             new-target (when (#{"replace" "create"} operation)
                          (.normalize (.resolve current id)))
             temporary (when new-target
                         (.resolve directory
                                   (str ".north-initialization-" id ".tmp")))]
         {"id" id
          "operation" operation
          "oldRawTarget" (some-> old-target str)
          "oldResolvedTarget" (some-> old-target .toAbsolutePath .normalize str)
          "newRawTarget" (some-> new-target str)
          "newResolvedTarget" (some-> new-target .toAbsolutePath .normalize str)
          "temporaryPath" (some-> temporary .toAbsolutePath .normalize str)}))
     ids)))

(defn- initialization-receipt-content [activation initialization entries]
  (let [ids-for (fn [operation]
                  (vec (sort (for [entry entries
                                   :when (= operation (get entry "operation"))]
                               (get entry "id")))))]
    (str
     (json/generate-string
      {"schema" initialization-receipt-schema
       "initializationId" (get-in initialization ["document" "id"])
       "initializationDigest" (:digest initialization)
       "catalogDigest" (get activation "catalogDigest")
       "generationId" (get activation "generationId")
       "permissions" (get activation "permissions")
       "adopted" (ids-for "replace")
       "created" (ids-for "create")
       "retired" (ids-for "remove")}
      {:pretty true})
     "\n")))

(defn- prepare-codex-links! [activation initialization]
  (let [root (ensure-root!)
        directory (codex-skills-dir)
        current (.toAbsolutePath (.resolve root "current/skills/shared"))
        current-pointer (.resolve root "current")
        manifest (.resolve root "codex-managed-skills.json")
        receipt (.resolve root "initialization-receipt.json")]
    (when (exists-no-follow? directory)
      (when (or (java.nio.file.Files/isSymbolicLink directory)
                (not (java.nio.file.Files/isDirectory
                      directory (into-array java.nio.file.LinkOption
                                            [java.nio.file.LinkOption/NOFOLLOW_LINKS]))))
        (fail "Codex skills path must be a real directory"
              {:path (str directory)})))
    (java.nio.file.Files/createDirectories
     directory (make-array java.nio.file.attribute.FileAttribute 0))
    (when (or (java.nio.file.Files/isSymbolicLink directory)
              (not (java.nio.file.Files/isDirectory
                    directory (into-array java.nio.file.LinkOption
                                          [java.nio.file.LinkOption/NOFOLLOW_LINKS]))))
      (fail "Codex skills path must be a real directory"
            {:path (str directory)}))
    (let [previous (read-managed-skill-ids manifest)
          current? (exists-no-follow? current-pointer)
          receipt? (exists-no-follow? receipt)]
      (if initialization
        (when (or current? (some? previous) receipt?)
          (fail "agent initialization is one-shot and requires no managed generation"
                {:current current? :manifest (some? previous) :receipt receipt?}))
        (when (not= current? (some? previous))
          (fail "agent activation current pointer and Codex manifest disagree"
                {:current current? :manifest (some? previous)})))
      (let [previous (or previous #{})
            active (set (map #(get % "unitId") (skill-plan activation "shared")))
            known-skills (set (for [unit (get activation "units")
                                    :when (= "skill" (get unit "kind"))]
                                (get unit "id")))
            expected (fn [id] (.normalize (.resolve current id)))
            skill-links (:skill-links initialization)
            initialization-entries
            (when initialization
              (initialization-plan activation initialization root directory))]
        (when initialization
          (doseq [[id _entry] skill-links
                  :let [path (.resolve directory id)]]
            (when-not (exists-no-follow? path)
              (fail (str "initial Codex skill link is missing: " id)
                    {:id id :path (str path)}))))
        (doseq [id known-skills
                :let [path (.resolve directory id)]
                :when (exists-no-follow? path)]
          (if initialization
            (let [entry (get skill-links id)]
              (when-not (and (= "adopt" (get entry "action"))
                             (= (raw-link-target path) (get entry "target")))
                (fail (str "Codex skill path collides with an unowned entry: " id)
                      {:id id :path (str path)
                       :actual (some-> (raw-link-target path) str)})))
            (when-not (and (previous id)
                           (= (resolved-link-target path) (expected id)))
              (fail (str "Codex skill path collides with an unowned entry: " id)
                    {:id id :path (str path)}))))
        (when initialization
          (doseq [[id entry] skill-links
                  :when (= "retire" (get entry "action"))
                  :let [path (.resolve directory id)]
                  :when (exists-no-follow? path)]
            (assert-literal-link-target!
             path (get entry "target")
             "obsolete initial Codex skill link has an unexpected target" id)))
        (doseq [id previous
                :let [path (.resolve directory id)]
                :when (exists-no-follow? path)]
          (assert-link-target! path (expected id)
                               "managed Codex skill link was replaced outside North" id))
        (let [existing? #(exists-no-follow? (.resolve directory %))
              transitions
              (into (sorted-map)
                    (for [id (sort active)]
                      [id (let [path (.resolve directory id)]
                            (if (existing? id)
                              {:mode :replace
                               :old-raw-target
                               (java.nio.file.Files/readSymbolicLink path)
                               :old-target (resolved-link-target path)}
                              {:mode :create}))]))
              transitions (if initialization
                            transitions
                            (into (sorted-map)
                                  (remove (fn [[id _]] (existing? id)))
                                  transitions))
              inactive-managed
              (if initialization
                (for [id known-skills
                      :when (and (not (active id)) (existing? id))]
                  id)
                (remove active previous))
              explicitly-retired
              (when initialization
                (for [[id entry] skill-links
                      :when (and (= "retire" (get entry "action")) (existing? id))]
                  id))
              removals
              (into (sorted-map)
                    (for [id (sort (distinct (concat inactive-managed explicitly-retired)))
                          :let [path (.resolve directory id)]
                          :when (exists-no-follow? path)]
                      [id {:old-raw-target
                           (java.nio.file.Files/readSymbolicLink path)
                           :old-target (resolved-link-target path)}]))
              link-temps
              (into {}
                    (for [id (keys transitions)
                          :let [temporary
                                (.resolve
                                 directory
                                 (if initialization
                                   (str ".north-initialization-" id ".tmp")
                                   (str ".north-" id "-"
                                        (java.util.UUID/randomUUID) ".tmp")))]]
                      [id temporary]))
              manifest-temp
              (.resolve root
                        (if initialization
                          ".codex-managed-skills.initialization.tmp"
                          (str ".codex-managed-skills-"
                               (java.util.UUID/randomUUID) ".tmp")))
              receipt-temp (when initialization
                             (.resolve root
                                       ".initialization-receipt.initialization.tmp"))]
          {:directory directory :manifest manifest :manifest-temp manifest-temp
           :manifest-content
           (managed-manifest-content active)
           :receipt receipt :receipt-temp receipt-temp
           :receipt-content
           (when initialization
             (initialization-receipt-content
              activation initialization initialization-entries))
           :transitions transitions :removals removals :expected expected
           :link-temps link-temps :initialization initialization
           :known-skills known-skills :active active
           :initialization-entries initialization-entries})))))

(declare cleanup-codex-temps!)

(defn- atomic-write-new-file! [path content stage]
  (let [scratch (.resolve (.getParent path)
                          (str ".north-write-" (java.util.UUID/randomUUID) ".tmp"))]
    (try
      (java.nio.file.Files/write
       scratch (.getBytes content java.nio.charset.StandardCharsets/UTF_8)
       (into-array java.nio.file.OpenOption
                   [java.nio.file.StandardOpenOption/CREATE_NEW
                    java.nio.file.StandardOpenOption/WRITE
                    java.nio.file.LinkOption/NOFOLLOW_LINKS]))
      (when stage (*codex-publication-stage!* stage nil))
      (java.nio.file.Files/move
       scratch path
       (into-array java.nio.file.CopyOption
                   [java.nio.file.StandardCopyOption/ATOMIC_MOVE]))
      path
      (catch Throwable error
        (when (and (java.nio.file.Files/isRegularFile
                    scratch
                    (into-array java.nio.file.LinkOption
                                [java.nio.file.LinkOption/NOFOLLOW_LINKS]))
                   (= (sha256 (slurp (str scratch))) (sha256 content)))
          (java.nio.file.Files/deleteIfExists scratch))
        (throw error)))))

(defn- stage-codex-temps!
  [{:keys [link-temps expected manifest-temp manifest-content
           receipt-temp receipt-content] :as plan}]
  (try
    (doseq [[id temporary] link-temps]
      (java.nio.file.Files/createSymbolicLink
       temporary (expected id)
       (make-array java.nio.file.attribute.FileAttribute 0)))
    (atomic-write-new-file! manifest-temp manifest-content
                            :manifest-temporary-staged)
    (when receipt-temp
      (atomic-write-new-file! receipt-temp receipt-content
                              :receipt-temporary-staged))
    plan
    (catch Throwable error
      (cleanup-codex-temps! plan)
      (throw error))))

(defn- cleanup-codex-temps!
  [{:keys [link-temps expected manifest-temp manifest-content
           receipt-temp receipt-content]}]
  (doseq [[id temporary] link-temps]
    (when (= (raw-link-target temporary) (expected id))
      (java.nio.file.Files/deleteIfExists temporary)))
  (when (and (java.nio.file.Files/isRegularFile
              manifest-temp
              (into-array java.nio.file.LinkOption
                          [java.nio.file.LinkOption/NOFOLLOW_LINKS]))
             (= (sha256 (slurp (str manifest-temp))) (sha256 manifest-content)))
    (java.nio.file.Files/deleteIfExists manifest-temp))
  (when (and receipt-temp
             (java.nio.file.Files/isRegularFile
              receipt-temp
              (into-array java.nio.file.LinkOption
                          [java.nio.file.LinkOption/NOFOLLOW_LINKS]))
             (= (sha256 (slurp (str receipt-temp))) (sha256 receipt-content)))
    (java.nio.file.Files/deleteIfExists receipt-temp)))

(defn- assert-staged-file! [path content label]
  (when-not (and (java.nio.file.Files/isRegularFile
                  path
                  (into-array java.nio.file.LinkOption
                              [java.nio.file.LinkOption/NOFOLLOW_LINKS]))
                 (= (sha256 (slurp (str path))) (sha256 content)))
    (fail (str label " changed during publication") {:path (str path)})))

(defn- initialization-transaction-path [root]
  (.resolve root "initialization-transaction.json"))

(defn- generation-name [activation]
  (str "gen-" (subs (get activation "generationId") (count "sha256:"))))

(defn- initialization-transaction-document
  [activation initialization root directory]
  (let [entries (initialization-plan activation initialization root directory)
        active (set (map #(get % "unitId") (skill-plan activation "shared")))
        manifest-content (managed-manifest-content active)
        receipt-content (initialization-receipt-content
                         activation initialization entries)]
    {"schema" initialization-transaction-schema
     "initializationId" (get-in initialization ["document" "id"])
     "initializationDigest" (:digest initialization)
     "catalogDigest" (get activation "catalogDigest")
     "generationId" (get activation "generationId")
     "codexSkillsDirectory" (str (.normalize (.toAbsolutePath directory)))
     "current" {"path" (str (.resolve root "current"))
                "rawTarget" (generation-name activation)
                "resolvedTarget" (str (.resolve root (generation-name activation)))}
     "manifest" {"path" (str (.resolve root "codex-managed-skills.json"))
                 "temporaryPath"
                 (str (.resolve root ".codex-managed-skills.initialization.tmp"))
                 "contentDigest" (sha256 manifest-content)}
     "receipt" {"path" (str (.resolve root "initialization-receipt.json"))
                "temporaryPath"
                (str (.resolve root ".initialization-receipt.initialization.tmp"))
                "contentDigest" (sha256 receipt-content)}
     "entries" entries}))

(defn- actual-initialization-entry [plan expected-entry]
  (let [id (get expected-entry "id")
        transition (get-in plan [:transitions id])
        removal (get-in plan [:removals id])
        operation (cond
                    transition (name (:mode transition))
                    removal "remove"
                    :else "absent")
        old-target (or (:old-raw-target transition)
                       (:old-raw-target removal))
        new-target (when transition ((:expected plan) id))
        temporary (get-in plan [:link-temps id])]
    {"id" id
     "operation" operation
     "oldRawTarget" (some-> old-target str)
     "oldResolvedTarget"
     (some-> old-target (#(.resolve (:directory plan) %)) .toAbsolutePath .normalize str)
     "newRawTarget" (some-> new-target str)
     "newResolvedTarget" (some-> new-target .toAbsolutePath .normalize str)
     "temporaryPath" (some-> temporary .toAbsolutePath .normalize str)}))

(defn- verify-initialization-plan! [plan transaction]
  (doseq [expected (get transaction "entries")]
    (let [actual (actual-initialization-entry plan expected)]
      (when-not (= expected actual)
        (fail (str "agent initialization plan disagrees for " (get expected "id"))
              {:expected expected :actual actual}))))
  (when-not (= (get-in transaction ["manifest" "contentDigest"])
               (sha256 (:manifest-content plan)))
    (fail "agent initialization manifest plan disagrees" {}))
  (when-not (= (get-in transaction ["receipt" "contentDigest"])
               (sha256 (:receipt-content plan)))
    (fail "agent initialization receipt plan disagrees" {}))
  (doseq [path (concat (vals (:link-temps plan))
                       [(:manifest-temp plan) (:receipt-temp plan)])]
    (when (exists-no-follow? path)
      (fail "agent initialization temporary path is already occupied"
            {:path (str path)})))
  plan)

(defn- write-initialization-transaction! [root transaction]
  (let [path (initialization-transaction-path root)
        content (str (json/generate-string transaction {:pretty true}) "\n")]
    (when (exists-no-follow? path)
      (fail "an interrupted agent initialization requires exact replay"
            {:path (str path)}))
    (atomic-write-new-file! path content :transaction-temporary-staged)))

(defn- read-initialization-transaction [root]
  (let [path (initialization-transaction-path root)]
    (when (exists-no-follow? path)
      (when-not (java.nio.file.Files/isRegularFile
                 path (into-array java.nio.file.LinkOption
                                  [java.nio.file.LinkOption/NOFOLLOW_LINKS]))
        (fail "agent initialization transaction must be a regular file"
              {:path (str path)}))
      (try
        (json/parse-string (slurp (str path)))
        (catch Exception error
          (fail (str "cannot read agent initialization transaction: "
                     (.getMessage error))
                {:path (str path)}))))))

(defn- link-recovery-state [path]
  (cond
    (not (exists-no-follow? path)) ["absent" nil]
    (java.nio.file.Files/isSymbolicLink path) ["link" (str (raw-link-target path))]
    :else ["other" nil]))

(defn- file-recovery-state [path]
  (cond
    (not (exists-no-follow? path)) ["absent" nil]
    (java.nio.file.Files/isRegularFile
     path (into-array java.nio.file.LinkOption
                      [java.nio.file.LinkOption/NOFOLLOW_LINKS]))
    ["file" (sha256 (slurp (str path)))]
    :else ["other" nil]))

(defn- assert-recovery-state! [label actual allowed]
  (when-not (contains? allowed actual)
    (fail (str "interrupted agent initialization has unexpected " label)
          {:actual actual :allowed allowed})))

(def ^:private north-write-scratch-pattern
  #"\.north-write-[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\.tmp")

(defn- cleanup-journaled-write-scratch! [root transaction]
  (let [content-digests
        #{(get-in transaction ["manifest" "contentDigest"])
          (get-in transaction ["receipt" "contentDigest"])}]
    (with-open [stream (java.nio.file.Files/list root)]
      (doseq [path (iterator-seq (.iterator stream))
              :when (and (re-matches north-write-scratch-pattern
                                     (str (.getFileName path)))
                         (java.nio.file.Files/isRegularFile
                          path
                          (into-array java.nio.file.LinkOption
                                      [java.nio.file.LinkOption/NOFOLLOW_LINKS]))
                         (content-digests (sha256 (slurp (str path)))))]
        (java.nio.file.Files/deleteIfExists path)))))

(defn- recover-initialization! [root activation initialization transaction]
  (let [directory (codex-skills-dir)
        expected (initialization-transaction-document
                  activation initialization root directory)]
    (when-not (= expected transaction)
      (fail "interrupted agent initialization requires the exact initialization input"
            {:expectedInitializationDigest (get expected "initializationDigest")
             :actualInitializationDigest (get transaction "initializationDigest")}))
    (when-not (and (exists-no-follow? directory)
                   (java.nio.file.Files/isDirectory
                    directory (into-array java.nio.file.LinkOption
                                          [java.nio.file.LinkOption/NOFOLLOW_LINKS]))
                   (not (java.nio.file.Files/isSymbolicLink directory)))
      (fail "interrupted initialization Codex skills directory changed"
            {:path (str directory)}))
    (let [current (get transaction "current")
          current-path (.toPath (io/file (get current "path")))
          manifest (get transaction "manifest")
          manifest-path (.toPath (io/file (get manifest "path")))
          manifest-temp (.toPath (io/file (get manifest "temporaryPath")))
          receipt (get transaction "receipt")
          receipt-path (.toPath (io/file (get receipt "path")))
          receipt-temp (.toPath (io/file (get receipt "temporaryPath")))]
      (assert-recovery-state!
       "current pointer" (link-recovery-state current-path)
       #{["absent" nil] ["link" (get current "rawTarget")]})
      (doseq [[label path document]
              [["manifest" manifest-path manifest]
               ["receipt" receipt-path receipt]]]
        (assert-recovery-state!
         label (file-recovery-state path)
         #{["absent" nil] ["file" (get document "contentDigest")]}))
      (doseq [[label path document]
              [["manifest temporary" manifest-temp manifest]
               ["receipt temporary" receipt-temp receipt]]]
        (assert-recovery-state!
         label (file-recovery-state path)
         #{["absent" nil] ["file" (get document "contentDigest")]}))
      (doseq [entry (get transaction "entries")]
        (let [id (get entry "id")
              path (.resolve directory id)
              old ["link" (get entry "oldRawTarget")]
              new ["link" (get entry "newRawTarget")]
              absent ["absent" nil]
              allowed (case (get entry "operation")
                        "replace" #{old new}
                        "create" #{absent new}
                        "remove" #{old absent}
                        "absent" #{absent})]
          (assert-recovery-state! (str "Codex skill " id)
                                  (link-recovery-state path) allowed)
          (when-let [temporary-path (get entry "temporaryPath")]
            (assert-recovery-state!
             (str "Codex skill temporary " id)
             (link-recovery-state (.toPath (io/file temporary-path)))
             #{absent new}))))
      ;; Every state is validated before recovery writes begin. Each recovery
      ;; step moves an entry to another state accepted by the same journal.
      (cleanup-journaled-write-scratch! root transaction)
      (doseq [entry (get transaction "entries")]
        (let [id (get entry "id")
              path (.resolve directory id)
              state (link-recovery-state path)
              old-target (some-> (get entry "oldRawTarget") io/file .toPath)]
          (case (get entry "operation")
            "replace" (when (= state ["link" (get entry "newRawTarget")])
                        (atomic-link! path old-target true))
            "create" (when (= state ["link" (get entry "newRawTarget")])
                       (java.nio.file.Files/delete path))
            "remove" (when (= state ["absent" nil])
                       (atomic-link! path old-target false))
            "absent" nil)
          (when-let [temporary-path (get entry "temporaryPath")]
            (java.nio.file.Files/deleteIfExists
             (.toPath (io/file temporary-path))))))
      (java.nio.file.Files/deleteIfExists current-path)
      (doseq [path [manifest-path manifest-temp receipt-path receipt-temp]]
        (java.nio.file.Files/deleteIfExists path))
      (java.nio.file.Files/delete
       (initialization-transaction-path root)))))

(defn- commit-codex-links!
  [{:keys [directory manifest manifest-temp receipt receipt-temp transitions
           removals expected link-temps initialization manifest-content
           receipt-content] :as plan}]
  (let [transitioned (atom [])
        removed (atom [])
        receipt-committed? (atom false)]
    (try
      (doseq [[id {:keys [mode old-target]}] transitions
              :let [path (.resolve directory id)]]
        (if (= mode :replace)
          (assert-link-target! path old-target
                               "initial Codex skill link changed during initialization" id)
          (when (exists-no-follow? path)
            (fail (str "Codex skill path appeared during publication: " id)
                  {:id id :path (str path)})))
        (assert-literal-link-target!
         (get link-temps id) (expected id)
         "staged Codex skill link changed during publication" id)
        (java.nio.file.Files/move
         (get link-temps id) path
         (into-array java.nio.file.CopyOption
                     (cond-> [java.nio.file.StandardCopyOption/ATOMIC_MOVE]
                       (= mode :replace)
                       (conj java.nio.file.StandardCopyOption/REPLACE_EXISTING))))
        (swap! transitioned conj id)
        (*codex-publication-stage!* :link-transitioned id))
      (doseq [[id {:keys [old-target]}] removals
              :let [path (.resolve directory id)]]
        (assert-link-target! path old-target
                             "managed Codex skill link changed during publication" id)
        (java.nio.file.Files/delete path)
        (swap! removed conj id)
        (*codex-publication-stage!* :link-retired id))
      (when initialization
        (assert-staged-file! receipt-temp receipt-content
                             "staged initialization receipt")
        (java.nio.file.Files/move
         receipt-temp receipt
         (into-array java.nio.file.CopyOption
                     [java.nio.file.StandardCopyOption/ATOMIC_MOVE]))
        (reset! receipt-committed? true)
        (*codex-publication-stage!* :receipt-transitioned nil))
      (assert-staged-file! manifest-temp manifest-content
                           "staged Codex manifest")
      (java.nio.file.Files/move
       manifest-temp manifest
       (into-array java.nio.file.CopyOption
                   [java.nio.file.StandardCopyOption/ATOMIC_MOVE
                    java.nio.file.StandardCopyOption/REPLACE_EXISTING]))
      (*codex-publication-stage!* :manifest-transitioned nil)
      (catch Throwable error
        (when @receipt-committed?
          (java.nio.file.Files/deleteIfExists receipt))
        (doseq [id (reverse @removed)
                :let [path (.resolve directory id)
                      target (get-in removals [id :old-raw-target])]]
          (when-not (exists-no-follow? path)
            (java.nio.file.Files/createSymbolicLink
             path target (make-array java.nio.file.attribute.FileAttribute 0))))
        (doseq [id (reverse @transitioned)
                :let [path (.resolve directory id)
                      {:keys [mode old-raw-target]} (get transitions id)]]
          (when (= (resolved-link-target path) (expected id))
            (if (= mode :create)
              (java.nio.file.Files/deleteIfExists path)
              (atomic-link! path old-raw-target true))))
        (cleanup-codex-temps! plan)
        (throw error)))
    plan))

(defn- replace-current! [root target]
  (let [current (.resolve root "current")]
    (if target
      (let [pointer (.resolve root (str ".current-" (java.util.UUID/randomUUID) ".tmp"))]
        (try
          (java.nio.file.Files/createSymbolicLink
           pointer target (make-array java.nio.file.attribute.FileAttribute 0))
          (java.nio.file.Files/move
           pointer current
           (into-array java.nio.file.CopyOption
                       [java.nio.file.StandardCopyOption/ATOMIC_MOVE
                        java.nio.file.StandardCopyOption/REPLACE_EXISTING]))
          (catch Throwable error
            (java.nio.file.Files/deleteIfExists pointer)
            (throw error))))
      (java.nio.file.Files/deleteIfExists current))))

(defn publish!
  ([activation] (publish! activation nil))
  ([activation initialization]
   (let [root (ensure-root!)
         interrupted (read-initialization-transaction root)]
     (when (and interrupted (nil? initialization))
       (fail (str "interrupted agent initialization requires exact replay with "
                  "NORTH_AGENT_INITIALIZATION")
             {:path (str (initialization-transaction-path root))}))
     (when interrupted
       (recover-initialization! root activation initialization interrupted))
     (let [links (prepare-codex-links! activation initialization)
           generation (stage-generation! activation)
           transaction (when initialization
                         (initialization-transaction-document
                          activation initialization root (codex-skills-dir)))
           prior (when (java.nio.file.Files/isSymbolicLink (.resolve root "current"))
                   (java.nio.file.Files/readSymbolicLink (.resolve root "current")))]
       (when initialization
         (verify-initialization-plan! links transaction)
         (write-initialization-transaction! root transaction))
       (try
         (stage-codex-temps! links)
         (replace-current! root (.getFileName generation))
         (*codex-publication-stage!* :current-transitioned nil)
         (try
           (commit-codex-links! links)
           (when initialization
             (java.nio.file.Files/delete
              (initialization-transaction-path root)))
           activation
           (catch Throwable error
             (replace-current! root prior)
             (throw error)))
         (catch Throwable error
           (cleanup-codex-temps! links)
           (when initialization
             (try
               (when-let [pending (read-initialization-transaction root)]
                 (recover-initialization! root activation initialization pending))
               (catch Throwable recovery-error
                 (.addSuppressed error recovery-error))))
           (throw error)))))))

(defn sync!
  ([] (sync! (not-empty (System/getenv "NORTH_AGENT_INITIALIZATION"))))
  ([initialization-path]
   (with-publication-lock
     (fn []
       (let [catalog (load-catalog)
             initialization (when initialization-path
                              (load-initialization catalog initialization-path))
             permissions (if initialization
                           (:permissions initialization)
                           (current-permissions catalog))
             activation (compile-activation catalog permissions)]
         (publish! activation initialization))))))

(defn change-permissions! [changes]
  (with-publication-lock
    (fn []
      (let [catalog (load-catalog)
            permissions (current-permissions catalog)]
        (doseq [[id permission] changes]
          (when-not (contains? (:by-id catalog) id)
            (fail (str "unknown unit: " id) {:id id}))
          (when-not (re-matches permission-pattern permission)
            (fail (str "invalid permission for " id ": " permission)
                  {:id id :permission permission})))
        (publish! (compile-activation catalog (merge permissions changes)))))))

(defn unit-path [id]
  (let [catalog (load-catalog)
        unit (get (:by-id catalog) id)]
    (when-not unit (fail (str "unknown unit: " id) {:id id}))
    (str (owner-path (get unit "owner") (str "unit " id)))))

(defn unit [id]
  (let [activation (or (current-activation)
                       (compile-activation (load-catalog)))
        result (some #(when (= id (get % "id")) %) (get activation "units"))]
    (when-not result (fail (str "unknown unit: " id) {:id id}))
    result))
