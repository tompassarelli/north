(ns north.agent-catalog
  (:require [cheshire.core :as json]
            [clojure.java.io :as io]
            [clojure.java.shell :as shell]
            [clojure.string :as str]))

(def catalog-schema "north.agent-catalog/v1")
(def activation-schema "north.agent-activation/v1")
(def permission-schema "north.agent-permissions/v1")

(def ^:private unit-id-pattern #"[a-z0-9][a-z0-9-]*")
(def ^:private kinds #{"skill" "hook" "set"})
(def ^:private unit-fields
  #{"id" "kind" "title" "triggerDescription" "category" "seedPermission"
    "owner" "members" "supports" "distributions"})
(def ^:private distribution-types
  #{"skill" "instructions" "hook" "agentTemplates" "providerAdapter"
    "projectPackage"})
(def ^:private distribution-targets
  #{"shared" "codex" "code" "north" "bridge" "firn" "claude"})
(def ^:private permission-pattern
  #"(?:on|off)")
(def ^:private source-root
  (some-> *file* io/file .getCanonicalFile .getParentFile .getParentFile str))
(defonce ^:private publication-lock (Object.))
(defonce ^:private revision-cache (atom {}))

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
    (when-not (re-matches permission-pattern (or (get unit "seedPermission") ""))
      (fail (str "unit " id " has an invalid seed permission")
            {:permission (get unit "seedPermission")}))
    (owner-path (get unit "owner") (str "unit " id))
    (when-not (and (vector? (get unit "distributions"))
                   (seq (get unit "distributions")))
      (fail (str "unit " id " has no distributions") {:id id}))
    (when (and (= kind "set")
               (or (str/blank? (get unit "title" ""))
                   (str/blank? (get unit "triggerDescription" ""))))
      (fail (str "set " id " must declare title and triggerDescription") {:id id}))
    unit))

(defn- exact-set-cycles! [units by-id]
  (let [color (atom {})]
    (letfn [(visit [id stack]
              (when (= "set" (get-in by-id [id "kind"]))
                (case (get @color id)
                  :grey (let [start (.indexOf stack id)
                              cycle (conj (subvec stack start) id)]
                          (fail (str "catalog set cycle: " (str/join " -> " cycle))
                                {:cycle cycle}))
                  :black nil
                  (do
                    (swap! color assoc id :grey)
                    (doseq [member (get-in by-id [id "members"])]
                      (when (= "set" (get-in by-id [member "kind"]))
                        (visit member (conj stack id))))
                    (swap! color assoc id :black)))))]
      (doseq [unit units :when (= "set" (get unit "kind"))]
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
          (when (and (not= kind "set") (seq members))
            (fail (str "non-set unit " id " declares members") {:members members}))
          (doseq [member members]
            (when-not (contains? by-id member)
              (fail (str "set " id " names unknown member " member) {:id id :member member}))
            (when (and (broadly-distributed? unit)
                       (project-only? (get by-id member)))
              (fail (str "broadly distributed set " id
                         " contains project-only member " member)
                    {:id id :member member})))
          (when-not (and (vector? supports) (= (count supports) (count (distinct supports))))
            (fail (str "unit " id " has duplicate or invalid supports") {:supports supports}))
          (when (and (not= kind "hook") (seq supports))
            (fail (str "non-hook unit " id " declares supports") {:supports supports}))
          (doseq [supported supports]
            (when-not (#{"skill" "set"} (get-in by-id [supported "kind"]))
              (fail (str "hook " id " supports unknown or invalid unit " supported)
                    {:id id :supports supported}))
            (when (and (#{"skill" "set"} (get-in by-id [supported "kind"]))
                       (project-only? unit)
                       (broadly-distributed? (get by-id supported)))
              (fail (str "broadly distributed unit " supported
                         " cannot depend on project-only hook " id)
                    {:id id :supports supported})))))
      (exact-set-cycles! units by-id)
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

(defn seed-permissions [catalog]
  (into (sorted-map)
        (map (fn [unit] [(get unit "id") (get unit "seedPermission")]))
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
        unknown (sort (remove known (keys previous)))]
    (when (seq unknown)
      (fail (str "current activation contains unknown permissions: "
                 (str/join ", " unknown)) {:ids unknown}))
    (merge (seed-permissions catalog) previous)))

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
                   (when (= "set" (get unit "kind"))
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
                       source (owner-path owner (str "instructions " (get entry "unitId")))] ]
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

(defn codex-skills-dir []
  (.toPath (io/file (str (System/getenv "HOME") "/.codex/skills"))))

(defn- exists-no-follow? [path]
  (java.nio.file.Files/exists
   path (into-array java.nio.file.LinkOption
                    [java.nio.file.LinkOption/NOFOLLOW_LINKS])))

(defn- read-managed-skill-ids [manifest]
  (if-not (exists-no-follow? manifest)
    #{}
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

(defn- prepare-codex-compat! [activation]
  (let [root (ensure-root!)
        directory (codex-skills-dir)
        current (.toAbsolutePath (.resolve root "current/skills/shared"))
        manifest (.resolve root "codex-managed-skills.json")]
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
          active (set (map #(get % "unitId") (skill-plan activation "shared")))
          expected (fn [id] (.normalize (.resolve current id)))
          create (sort (remove #(exists-no-follow? (.resolve directory %)) active))
          stale (sort (remove active previous))]
      (doseq [id active
              :let [path (.resolve directory id)]
              :when (exists-no-follow? path)]
        (when-not (and (previous id)
                       (= (resolved-link-target path) (expected id)))
          (fail (str "Codex skill path collides with an unowned entry: " id)
                {:id id :path (str path)})))
      (doseq [id stale
              :let [path (.resolve directory id)]
              :when (exists-no-follow? path)]
        (when-not (= (resolved-link-target path) (expected id))
          (fail (str "managed Codex skill link was replaced outside North: " id)
                {:id id :path (str path)})))
      (let [link-temps
            (into {}
                  (for [id create
                        :let [temporary (.resolve directory
                                                  (str ".north-" id "-"
                                                       (java.util.UUID/randomUUID) ".tmp"))]]
                    (do
                      (java.nio.file.Files/createSymbolicLink
                       temporary (expected id)
                       (make-array java.nio.file.attribute.FileAttribute 0))
                      [id temporary])))
            manifest-temp (.resolve root
                                    (str ".codex-managed-skills-"
                                         (java.util.UUID/randomUUID) ".tmp"))]
        (try
          (spit (str manifest-temp)
                (str (json/generate-string
                      {"schema" "north.codex-managed-skills/v1"
                       "ids" (vec (sort active))}
                      {:pretty true}) "\n"))
          {:directory directory :manifest manifest :manifest-temp manifest-temp
           :active active :create create :stale stale :expected expected
           :link-temps link-temps}
          (catch Throwable error
            (doseq [temporary (vals link-temps)]
              (java.nio.file.Files/deleteIfExists temporary))
            (java.nio.file.Files/deleteIfExists manifest-temp)
            (throw error)))))))

(defn- cleanup-codex-temps! [{:keys [link-temps manifest-temp]}]
  (doseq [temporary (vals link-temps)]
    (java.nio.file.Files/deleteIfExists temporary))
  (java.nio.file.Files/deleteIfExists manifest-temp))

(defn- commit-codex-compat!
  [{:keys [directory manifest manifest-temp create stale expected link-temps] :as plan}]
  (let [created (atom [])
        deleted (atom [])]
    (try
      (doseq [id create]
        (java.nio.file.Files/move
         (get link-temps id) (.resolve directory id)
         (into-array java.nio.file.CopyOption
                     [java.nio.file.StandardCopyOption/ATOMIC_MOVE]))
        (swap! created conj id))
      (doseq [id stale
              :let [path (.resolve directory id)]
              :when (exists-no-follow? path)]
        (java.nio.file.Files/delete path)
        (swap! deleted conj id))
      (java.nio.file.Files/move
       manifest-temp manifest
       (into-array java.nio.file.CopyOption
                   [java.nio.file.StandardCopyOption/ATOMIC_MOVE
                    java.nio.file.StandardCopyOption/REPLACE_EXISTING]))
      (cleanup-codex-temps! plan)
      (catch Throwable error
        (doseq [id @created]
          (java.nio.file.Files/deleteIfExists (.resolve directory id)))
        (doseq [id @deleted]
          (java.nio.file.Files/createSymbolicLink
           (.resolve directory id) (expected id)
           (make-array java.nio.file.attribute.FileAttribute 0)))
        (cleanup-codex-temps! plan)
        (throw error)))))

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

(defn publish! [activation]
  (let [root (ensure-root!)
        compat (prepare-codex-compat! activation)
        prior (when (java.nio.file.Files/isSymbolicLink (.resolve root "current"))
                (java.nio.file.Files/readSymbolicLink (.resolve root "current")))]
    (try
      (let [generation (stage-generation! activation)]
        (replace-current! root (.getFileName generation))
        (try
          (commit-codex-compat! compat)
          activation
          (catch Throwable error
            (replace-current! root prior)
            (throw error))))
      (catch Throwable error
        (cleanup-codex-temps! compat)
        (throw error)))))

(defn sync! []
  (with-publication-lock
    (fn []
      (let [catalog (load-catalog)
            activation (compile-activation catalog)]
        (publish! activation)))))

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
