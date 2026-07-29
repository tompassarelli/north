#!/usr/bin/env bb
;; End-to-end contract for native constitution assembly. Every case uses an
;; isolated home, state file, source, and provider output.
(require '[babashka.process :as p]
         '[clojure.java.io :as io]
         '[clojure.string :as str])

(def root
  (.getCanonicalPath
   (io/file (.getParent (io/file (System/getProperty "babashka.file"))) "../..")))
(def cli (str root "/cli/config-cli.clj"))
(def tmp-dir
  (.toFile
   (java.nio.file.Files/createTempDirectory
    "north-config-context-test-"
    (make-array java.nio.file.attribute.FileAttribute 0))))
(def scratch-home (str tmp-dir "/home"))
(def state (str tmp-dir "/harness.conf"))
(def context-source (str tmp-dir "/AGENTS.md"))
(def output (str tmp-dir "/native/CLAUDE.md"))
(def checks (atom []))

(def preamble "# Context fixture\n\nFixture preamble.\n\n")
(def core-section
  (str "## Blocked ≠ stopped\n"
       "<!-- north-section: malformed-core · bucket: sideways -->\n\n"
       "CORE_SECTION\n\n"))
(def push-section
  (str "## Push freely\n"
       "<!-- north-section: push · bucket: write -->\n\n"
       "PUSH_SECTION\n\n"))
(def external-section
  (str "## External code — license first\n\n"
       "EXTERNAL_SECTION\n\n"))
(def orch-section
  (str "## Pre-edit gate — MANDATORY at task intake\n"
       "<!-- north-section: pre-edit-gate · bucket: orch -->\n\n"
       "ORCH_SECTION\n"))
(def fixture
  (str preamble core-section push-section external-section orch-section))

(defn check [label value]
  (swap! checks conj [label (boolean value)]))

(defn run-cli [& args]
  (apply p/shell
         {:out :string
          :err :string
          :continue true
          :extra-env {"HOME" scratch-home
                      "NORTH_HOME" root
                      "NORTH_HARNESS_STATE" state
                      "NORTH_CONTEXT_SOURCE" context-source
                      "NORTH_CONTEXT_OUTPUT" output}}
         (into ["bb" cli "context"] args)))

(defn stored [key]
  (let [prefix (str key "=")]
    (some->> (when (.isFile (io/file state)) (slurp state))
             str/split-lines
             (filter #(str/starts-with? % prefix))
             last
             (#(subs % (count prefix))))))

(defn seed-state! [text]
  (io/make-parents state)
  (spit state text))

(defn install-fixture! []
  (.mkdirs (io/file scratch-home))
  (spit context-source fixture))

(defn full-case []
  (let [applied (run-cli "apply")
        shown (run-cli "show")]
    (check "full apply succeeds" (zero? (:exit applied)))
    (check "default mode copies the source byte-for-byte"
           (= fixture (slurp output)))
    (check "show reports the full default"
           (and (zero? (:exit shown))
                (str/includes? (:out shown) "context = full")
                (str/includes? (:out shown) "push")
                (str/includes? (:out shown) "full")))))

(defn precedence-case []
  ;; Gated with no exclusions must reconstruct the source exactly.
  (seed-state! "context=gated\n")
  (let [identity (run-cli "apply")]
    (check "all-on gated apply succeeds" (zero? (:exit identity)))
    (check "all-on gated output is byte-identical" (= fixture (slurp output))))

  (let [bucket (run-cli "bucket" "off" "write")
        item (run-cli "on" "push")
        shown (run-cli "show")
        applied (run-cli "apply")
        expected (str preamble core-section push-section orch-section)]
    (check "bucket and section writes succeed"
           (and (zero? (:exit bucket)) (zero? (:exit item))))
    (check "specific changes activate gated mode"
           (= "gated" (stored "context")))
    (check "shared precedence is section over bucket"
           (and (re-find #"(?m)^  push\s+write\s+on\s+item\s+tagged$" (:out shown))
                (re-find #"(?m)^  external-code\s+write\s+off\s+bucket\s+fallback$" (:out shown))))
    (check "malformed tags use the compatibility heading table"
           (re-find #"(?m)^  blocked\s+core\s+on\s+default\s+fallback$" (:out shown)))
    (check "apply succeeds" (zero? (:exit applied)))
    (check "only the excluded section span differs"
           (= expected (slurp output)))))

(defn ttl-case []
  (seed-state!
   (str "context=gated\n"
        "context.bucket.write=off\n"
        "context.section.push=off:until=2020-01-01T00:00:00Z\n"))
  (let [applied (run-cli "apply")
        shown (run-cli "show")]
    (check "expired TTL apply succeeds" (zero? (:exit applied)))
    (check "expired item TTL restores on and stops broader fallback"
           (and (str/includes? (slurp output) "PUSH_SECTION")
                (re-find #"(?m)^  push\s+write\s+on\s+item\s+tagged$" (:out shown)))))

  (seed-state!
   (str "context=gated\n"
        "context.bucket.write=on\n"
        "context.section.push=off:until=2099-01-01T00:00:00Z\n"))
  (let [applied (run-cli "apply")
        shown (run-cli "show")]
    (check "future TTL apply succeeds" (zero? (:exit applied)))
    (check "future item TTL overrides an on bucket"
           (and (not (str/includes? (slurp output) "PUSH_SECTION"))
                (re-find #"(?m)^  push\s+write\s+off\s+item\s+tagged$" (:out shown))))))

(defn symlink-case []
  (let [sentinel (str tmp-dir "/provider-neutral-source")]
    (java.nio.file.Files/deleteIfExists (.toPath (io/file state)))
    (java.nio.file.Files/deleteIfExists (.toPath (io/file output)))
    (io/make-parents output)
    (spit sentinel "SOURCE_MUST_NOT_CHANGE")
    (java.nio.file.Files/createSymbolicLink
     (.toPath (io/file output))
     (.toPath (io/file sentinel))
     (make-array java.nio.file.attribute.FileAttribute 0))
    (let [applied (run-cli "apply")
          output-path (.toPath (io/file output))]
      (check "apply over a symlink succeeds" (zero? (:exit applied)))
      (check "provider output becomes a regular file"
             (and (not (java.nio.file.Files/isSymbolicLink output-path))
                  (java.nio.file.Files/isRegularFile
                   output-path
                   (make-array java.nio.file.LinkOption 0))))
      (check "replacement carries exact source bytes" (= fixture (slurp output)))
      (check "the former symlink target is untouched"
             (= "SOURCE_MUST_NOT_CHANGE" (slurp sentinel))))))

(def requested (or (first *command-line-args*) "all"))

(try
  (install-fixture!)
  (case requested
    "full" (full-case)
    "precedence" (precedence-case)
    "ttl" (ttl-case)
    "symlink" (symlink-case)
    "all" (do (full-case) (precedence-case) (ttl-case) (symlink-case))
    (do (binding [*out* *err*]
          (println "usage: bb cli/tests/config-context-test.clj [full|precedence|ttl|symlink|all]"))
        (System/exit 2)))
  (finally
    (doseq [file (reverse (file-seq tmp-dir))]
      (io/delete-file file true))))

(let [results @checks
      passed (count (filter second results))]
  (doseq [[label ok] results]
    (println (format "  [%s] %s" (if ok "PASS" "FAIL") label)))
  (println (format "\nconfig context (%s): %d / %d PASS"
                   requested passed (count results)))
  (System/exit (if (= passed (count results)) 0 1)))
