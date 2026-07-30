#!/usr/bin/env bb
(require '[babashka.process :as proc]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(load-file (str root "/cli/worktree-census.clj"))

(def checks (atom []))
(defn check [label value & [detail]]
  (swap! checks conj [label (boolean value) detail]))

(defn fact [tx op subject predicate value]
  (pr-str (array-map
           :tx tx
           :r value
           :frame "worktree-census-test"
           :p predicate
           :l subject
           :op op)))

(let [tmp (.toFile
           (java.nio.file.Files/createTempDirectory
            "north worktree census "
            (make-array java.nio.file.attribute.FileAttribute 0)))
      log (io/file tmp "facts.log")
      claimed-path (.getCanonicalPath
                    (doto (io/file tmp "wt-space and \"quote\"") .mkdirs))
      retracted-path (.getCanonicalPath
                      (doto (io/file tmp "wt-retracted") .mkdirs))]
  (try
    (spit log
          (str/join
           "\n"
           [(fact 1 "assert" "@lane-ignored" "title" "not a worktree")
            (fact 2 "assert" "@lane-claimed" "worktree" claimed-path)
            (fact 3 "assert" "@lane-retracted" "worktree" retracted-path)
            (fact 4 "retract" "@lane-retracted" "worktree" retracted-path)
            ""]))
    (with-redefs [proc/shell
                  (fn [& args]
                    (throw (ex-info "worktree census spawned a process"
                                    {:args args})))]
      (check "live worktree facts are selected without a subprocess"
             (= {claimed-path "@lane-claimed"}
                (north.worktree-census/claimed-worktrees (.getPath log)))))

    (let [calls (atom [])
          porcelain
          (str
           "worktree /repo/main checkout\n"
           "HEAD 1111111111111111111111111111111111111111\n"
           "branch refs/heads/main\n\n"
           "worktree /repo/wt path with spaces\n"
           "HEAD 2222222222222222222222222222222222222222\n"
           "branch refs/heads/feature\n"
           "locked held by test\n\n"
           "worktree /repo/bare store\n"
           "bare\n\n"
           "worktree /repo/detached path\n"
           "HEAD 3333333333333333333333333333333333333333\n"
           "detached\n"
           "prunable metadata missing\n")]
      (with-redefs [north.worktree-census/git
                    (fn [& args]
                      (swap! calls conj (vec args))
                      {:exit 0 :out porcelain :err ""})]
        (let [entries
              (north.worktree-census/registered-worktrees
               "/repo/main checkout")]
          (check "porcelain invocation keeps a root containing spaces atomic"
                 (= [["-C" "/repo/main checkout"
                      "worktree" "list" "--porcelain"]]
                    @calls))
          (check "porcelain records preserve source order"
                 (= ["/repo/main checkout"
                     "/repo/wt path with spaces"
                     "/repo/bare store"
                     "/repo/detached path"]
                    (mapv :path entries)))
          (check "branch, lock, bare, and detached records retain their meaning"
                 (= [{:path "/repo/main checkout"
                      :head "1111111111111111111111111111111111111111"
                      :branch "main"
                      :detached? false :locked? false :prunable? false}
                     {:path "/repo/wt path with spaces"
                      :head "2222222222222222222222222222222222222222"
                      :branch "feature"
                      :detached? false :locked? true :prunable? false}
                     {:path "/repo/bare store"
                      :head nil :branch nil
                      :detached? false :locked? false :prunable? false}
                     {:path "/repo/detached path"
                      :head "3333333333333333333333333333333333333333"
                      :branch nil
                      :detached? true :locked? false :prunable? true}]
                    entries)))))
    (let [worktrees-cli (slurp (io/file root "cli" "worktrees-cli.clj"))]
      (check "worktrees observability names the in-process EDN fold"
             (and (str/includes?
                   worktrees-cli
                   "Clojure EDN fold <live worktree facts>")
                  (not (str/includes?
                        worktrees-cli
                        "gawk <live worktree facts>")))))
    (finally
      (doseq [file (reverse (file-seq tmp))]
        (try (io/delete-file file true) (catch Throwable _ nil)))))

  (let [results @checks
        passed (count (filter second results))]
    (doseq [[label ok detail] results]
      (println (format "  [%s] %s" (if ok "PASS" "FAIL") label))
      (when (and (not ok) detail) (println (str "        " detail))))
    (println (format "\nworktree census: %d / %d PASS"
                     passed (count results)))
    (System/exit (if (= passed (count results)) 0 1))))
