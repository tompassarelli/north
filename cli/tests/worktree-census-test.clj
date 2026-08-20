#!/usr/bin/env bb
(require '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(load-file (str root "/cli/coord.clj"))
(load-file (str root "/cli/worktree-census.clj"))

(def checks (atom []))
(defn check [label value & [detail]]
  (swap! checks conj [label (boolean value) detail]))

(let [tmp (.toFile
           (java.nio.file.Files/createTempDirectory
            "north worktree census "
            (make-array java.nio.file.attribute.FileAttribute 0)))
      claimed-path (.getCanonicalPath
                    (doto (io/file tmp "worktrees" "space and \"quote\"") .mkdirs))]
  (try
    (let [calls (atom [])]
      (with-redefs [north.coord/bounded-query-in-domain
                    (fn [& args]
                      (swap! calls conj args)
                      {:rows [["@lane-claimed" claimed-path]
                              ["@lane-invalid" "relative/wt-invalid"]]})]
        (check "live worktree claims come from one bounded coordination query"
               (and (= {claimed-path "@lane-claimed"}
                       (north.worktree-census/claimed-worktrees 7977))
                    (= 1 (count @calls))
                    (= [7977 :coordination]
                       (vec (take 2 (first @calls))))
                    (= "worktree_claim"
                       (get-in (vec (first @calls)) [2 :find]))
                    (= "worktree"
                       (get-in (vec (first @calls))
                               [2 :rules 0 :body 0 :args 1]))
                    (= 4096 (last (first @calls)))))))

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
      (check "worktrees observability names the STORE RPC indexed query"
             (and (str/includes?
                   worktrees-cli
                   "STORE RPC indexed query predicate=worktree port=")
                  (not (str/includes?
                        worktrees-cli
                        "north.coord/expected-log")))))
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
