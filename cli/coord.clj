;; coord.clj — the ONE shared coordination substrate for the lodestar *-cli.clj
;; scripts (Foundation thread 019f100f Part B). Every CLI spoke the :7977 daemon
;; wire (:assert / :version / :retract / :resolved / :query) through a VERBATIM
;; copy of these helpers — 10 copies of send-op, 5 of assert!, 2 of retract!, and
;; ~11 single/multi resolved variants. One drift in any copy and the fleet's
;; coordination silently diverges. This is the single definition they all load.
;;
;; SEMANTICS UNCHANGED — this is pure de-duplication. The OCC verb redesign
;; (append!/put!/swap!) is a LATER thread (C); here assert!/retract! keep the
;; existing read-:version-then-CAS-with-retry behavior byte-for-byte.
;;
;; DUAL MODE (the schema-validate.clj precedent): load-file'd by a sibling CLI as a
;; library, OR run directly as a connectivity smoke. The main-guard keeps the CLI
;; dormant when another script loads us:
;;   bb cli/coord.clj <port>            -> prints the daemon's :version (a ping)
;; Load it sibling-relative so cwd never matters:
;;   (load-file (str (.getParent (io/file (System/getProperty "babashka.file"))) "/coord.clj"))
;; then call lodestar.coord/send-op (or rebind the local names you use).
(ns lodestar.coord
  (:require [clojure.edn :as edn] [clojure.java.io :as io]))

;; The canonical coordinator port. The CLIs take <port> as argv[0]; PORT is the
;; default/canonical reference (Part C's pred-cli + future callers read it).
(def PORT (or (System/getenv "LODESTAR_PORT") "7977"))

;; one request/response over the daemon socket: write one EDN op + newline, read
;; one EDN reply line. The atom every other helper is built from.
(defn send-op [port op]
  (with-open [s (java.net.Socket. "127.0.0.1" (int port))]
    (let [w (.getOutputStream s) r (io/reader (.getInputStream s))]
      (.write w (.getBytes (str (pr-str op) "\n"))) (.flush w)
      (edn/read-string (.readLine r)))))

;; the daemon's current global version (the OCC base read).
(defn cur-ver [port] (:version (send-op port {:op :version})))

;; OCC assert at the current :version; retry on reject (a concurrent write moved
;; the base). (str r) coerces — every caller already passes a string, so it is a
;; no-op for them, kept defensive. 4 tries (the dominant copy; concern was 3 —
;; strictly-more retries, identical success semantics).
(defn assert! [port te p r]
  (loop [tries 4]
    (let [res (send-op port {:op :assert :te te :p p :r (str r) :base (cur-ver port)})]
      (if (and (:reject res) (pos? tries)) (recur (dec tries)) res))))

(defn retract! [port te p r]
  (loop [tries 4]
    (let [res (send-op port {:op :retract :te te :p p :r (str r) :base (cur-ver port)})]
      (if (and (:reject res) (pos? tries)) (recur (dec tries)) res))))

;; single live value of (te,p)  (the resolved/one/rf variants collapse here).
(defn resolved [port te p] (:value (send-op port {:op :resolved :te te :p p})))
;; all live values of (te,p) — multi-valued  (the many/rmany variants).
(defn many     [port te p] (:values (send-op port {:op :resolved :te te :p p})))

(defn -main [& args]
  (let [port (Integer/parseInt (or (first args) PORT))]
    (prn (send-op port {:op :version}))))

(when (= *file* (System/getProperty "babashka.file"))
  (apply -main *command-line-args*))
