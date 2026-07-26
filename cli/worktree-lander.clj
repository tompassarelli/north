#!/usr/bin/env bb
;; Reconcile durable harvested lane refs after an explicit ff-only landing.
;; Only refs whose tips are already ancestors of main are deleted. Everything
;; else remains a recovery artifact and is printed for human/coordinator review.
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(defn git [repo & args]
  (apply proc/shell {:out :string :err :string :continue true} "git" "-C" repo args))

(defn die [message]
  (binding [*out* *err*] (println (str "worktree lander: " message)))
  (System/exit 2))

(defn canonical-repo [path]
  (let [result (git path "rev-parse" "--show-toplevel")]
    (when (zero? (:exit result))
      (.getCanonicalPath (io/file (str/trim (str (:out result))))))))

(defn refs [repo]
  (let [result (git repo "for-each-ref" "--format=%(refname)%09%(objectname)%09%(creatordate:iso8601)%09%(subject)"
                    "refs/heads/lane-lane-*")]
    (if-not (zero? (:exit result)) (die (str/trim (str (:err result))))
      (for [line (str/split-lines (str (:out result)))
            :let [[ref oid age subject] (str/split line #"\t" 4)]
            :when (and (re-matches #"refs/heads/lane-lane-[A-Za-z0-9][A-Za-z0-9._-]*" ref)
                       (re-matches #"[0-9a-f]{40,64}" oid))]
        {:ref ref :oid oid :age age :subject (or subject "")}))))

(defn landed? [repo oid]
  (zero? (:exit (git repo "merge-base" "--is-ancestor" oid "main"))))

(defn delete-landed! [repo {:keys [ref oid]}]
  ;; Compare-and-delete refuses a concurrent ref update instead of deleting it.
  (git repo "update-ref" "-d" ref oid))

(let [repo (canonical-repo (or (first *command-line-args*) "."))]
  (when-not repo (die "repository must be an existing Git checkout"))
  (doseq [entry (refs repo)]
    (if (landed? repo (:oid entry))
      (let [deleted (delete-landed! repo entry)]
        (if (zero? (:exit deleted))
          (println (str "LANDED DELETE " (:ref entry) " age=" (:age entry)
                        " subject=" (:subject entry)))
          (println (str "KEEP " (:ref entry) " age=" (:age entry)
                        " subject=" (:subject entry) " reason=delete-race-or-error"))))
      (println (str "UNLANDED KEEP " (:ref entry) " age=" (:age entry)
                    " subject=" (:subject entry))))))
