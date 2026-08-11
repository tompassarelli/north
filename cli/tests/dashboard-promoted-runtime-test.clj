#!/usr/bin/env bb
;; Doctor's two promote-channel readings: which north commit the runtime
;; selector points at, and how many rebuild asks exist only to adopt code.
(require '[babashka.process :as p]
         '[clojure.java.io :as io])

(def test-script (or (System/getProperty "babashka.file") *file*))

;; dashboard-cli's library guard is process-environment based because its public
;; entrypoint is also the executable. Re-enter once with the guard set.
(when-not (= "1" (System/getenv "NORTH_DASHBOARD_LIB"))
  (let [result @(p/process ["env" "NORTH_DASHBOARD_LIB=1" "bb" test-script]
                           {:out :string :err :string})]
    (print (:out result))
    (binding [*out* *err*] (print (:err result)))
    (flush)
    (System/exit (:exit result))))
(def root
  (-> test-script io/file .getCanonicalFile .getParentFile .getParentFile .getParent str))
(def dashboard-script (str root "/cli/dashboard-cli.clj"))

(System/setProperty "babashka.file" dashboard-script)
(try
  (load-file dashboard-script)
  (finally
    (System/setProperty "babashka.file" test-script)))

(def checks (atom []))

(defn check [label value]
  (let [ok (boolean value)]
    (swap! checks conj [label ok])
    (println (str (if ok "PASS " "FAIL ") label))))

(def revision "0123456789abcdef0123456789abcdef01234567")

(defn temp-root []
  (str (java.nio.file.Files/createTempDirectory
        "north-runtime-doctor" (into-array java.nio.file.attribute.FileAttribute []))))

(defn link! [from to]
  (java.nio.file.Files/createSymbolicLink
   (.toPath (io/file from)) (.toPath (io/file to))
   (into-array java.nio.file.attribute.FileAttribute [])))

;; The whole selector chain, exactly as bin/north-runtime publishes it: the
;; stable name resolves through the generation, and only the deployment
;; directory carries the revision.
(defn selector! [state deployment-name]
  (.mkdirs (io/file state "deployments" deployment-name))
  (.mkdirs (io/file state "generations/g"))
  (link! (str state "/generations/g/current") (str "../../deployments/" deployment-name))
  (link! (str state "/active") "generations/g")
  (link! (str state "/current") "active/current")
  state)

(let [absent (temp-root)]
  (check "an absent selector is the pre-promote state, not a failure"
         (= {:promoted? false} (promoted-runtime absent))))

(let [dangling (temp-root)]
  (link! (str dangling "/current") "active/current")
  (check "a dangling selector reads as no promote"
         (= {:promoted? false} (promoted-runtime dangling))))

(let [state (selector! (temp-root) revision)
      result (promoted-runtime state)]
  (check "a published selector reports the deployment's exact revision"
         (= revision (:revision result)))
  (check "a published selector reports promoted"
         (true? (:promoted? result)))
  (check "a published selector reports the deployment path"
         (= (.getCanonicalPath (io/file state "deployments" revision))
            (:deployment result))))

(let [state (selector! (temp-root) "not-a-revision")
      result (promoted-runtime state)]
  (check "a selector that names no revision is malformed, never promoted"
         (and (false? (:promoted? result)) (some? (:malformed result)))))

(let [failed (remove second @checks)]
  (println (str "dashboard promoted runtime: "
                (- (count @checks) (count failed))
                " / " (count @checks) " PASS"))
  (System/exit (if (empty? failed) 0 1)))
