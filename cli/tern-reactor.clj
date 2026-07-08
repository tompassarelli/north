#!/usr/bin/env bb
;; ============================================================================
;; tern-reactor.clj <port> [debounce-ms] — COORDINATOR AUTO-EXPORT.
;;
;; The threads/*.md files are a PROJECTION of the fact log, but freshness was
;; MANUAL (`tern export`/`heal`) and forbidden during concurrent work — so every
;; write that didn't self-render (`fram tell`, the MCP tell tool, and the CLI
;; spokes concern/presence/msg/lease that write via the daemon socket) left the
;; file lagging the log. That lag ACCUMULATED (348 stale facts in one day) until
;; a human ran `heal`, and doctor screamed DEGRADED at every boot for the benign
;; drift. This reactor kills the class at the root: it treats the coordinator's
;; commit stream as the trigger and re-projects touched threads automatically, so
;; files NEVER lag the log and no client ever has to remember to render.
;;
;; HOW: the daemon already firehoses every commit to :subscribe subscribers
;; (coord_daemon notify-subs!). We subscribe (nil filter = firehose), coalesce
;; a burst of commits behind a short debounce, then shell the SAME `tern heal` a
;; human runs — byte-identical to `tern export` (both render via fram.export/
;; thread-md) and FAIL-CLOSED on genuine hand edits (a human decides those). heal
;; self-scopes: it re-renders ONLY the files that diverge from the log, so a burst
;; of edits costs one flush, and an idle stream costs nothing.
;;
;; This needs NO change to the coordinator (fram) — it rides the existing
;; :subscribe seam. It is a standalone sidecar: start it alongside the daemon.
;;   FRAM_LOG / FRAM_THREADS / FRAM_PORT select the target state (same env
;;   `tern`/`fram-up` read); heal inherits them from our env.
;;
;;   bb cli/tern-reactor.clj 7977            # firehose :7977, 400ms debounce
;;   bb cli/tern-reactor.clj 7977 250        # tighter debounce
;;   tern reactor &                          # via the bin/tern wrapper (bg task)
;; ============================================================================
(require '[clojure.edn :as edn] '[clojure.java.io :as io] '[clojure.string :as str]
         '[babashka.process :as proc])

(def args *command-line-args*)
(def port (Integer/parseInt (or (first args) (System/getenv "FRAM_PORT") "7977")))
(def debounce-ms (Integer/parseInt (or (second args) "400")))

;; bin/tern is a sibling of this cli/ dir: <repo>/cli/tern-reactor.clj -> <repo>/bin/tern
(def tern-bin
  (-> (io/file (System/getProperty "babashka.file"))
      .getParentFile .getParentFile (io/file "bin" "tern") .getPath))

;; Coordination-EPHEMERAL subjects: never projected to a thread .md AND written at
;; tool-call frequency (presence leases, session stamps, per-run costs, messages,
;; command envelopes, agent/role registry). Skipping them keeps heal firing only on
;; REAL thread edits instead of on every heartbeat — the reactor's whole cost budget.
(def ephemeral-prefixes ["@lease:" "@session:" "@run:" "@cmd:" "@agent:" "@role:"])
(defn ephemeral? [l]
  (and (string? l) (boolean (some #(str/starts-with? l %) ephemeral-prefixes))))

(def last-commit (atom 0))   ; wall-clock of the most recent projected-relevant commit
(def dirty       (atom false))
(def running     (atom false))

(defn heal! []
  ;; Shell the SAME `tern heal` a human runs — byte-identical projection, fail-closed
  ;; on hand edits, reads the flat log directly (no daemon dependency). FRAM_LOG/
  ;; FRAM_THREADS/FRAM_PORT are inherited from our env, pinning the target state.
  (try
    (let [r   (proc/shell {:out :string :err :string :continue true} tern-bin "heal")
          out (str/trim (str (:out r) (when (seq (:err r)) (str "\n" (:err r)))))]
      (when (seq out)
        (println (str "[reactor] " (str/replace out #"\n+" " | ")))
        (flush)))
    (catch Throwable t
      (println (str "[reactor] heal error: " (.getMessage t))) (flush))))

;; Flusher: once a burst goes quiet for debounce-ms, project. Coalesced — only one
;; heal in flight; commits arriving mid-heal re-arm dirty for the next quiet window.
(defn flusher []
  (loop []
    (Thread/sleep 100)
    (when (and @dirty (not @running)
               (>= (- (System/currentTimeMillis) @last-commit) debounce-ms))
      (reset! dirty false)
      (reset! running true)
      (try (heal!) (finally (reset! running false))))
    (recur)))

(defn mark! [l]
  (when-not (ephemeral? l)
    (reset! last-commit (System/currentTimeMillis))
    (reset! dirty true)))

(defn subscribe-once
  "Open one subscription and pump commit events until the socket drops. Returns on
   disconnect (daemon bounce / restart) so -main can reconnect."
  []
  (with-open [s (java.net.Socket. "127.0.0.1" (int port))]
    (.setSoTimeout s 0)                 ; long-lived: block on pushes, no read timeout
    (let [w (.getOutputStream s)
          r (io/reader (.getInputStream s))]
      (.write w (.getBytes "{:op :subscribe}\n")) (.flush w)
      (.readLine r)                     ; consume the {:subscribed <seq>} handshake
      (loop []
        (when-let [line (.readLine r)]
          (let [ev (try (edn/read-string line) (catch Throwable _ nil))]
            (when (and (map? ev) (= (:event ev) :commit))
              (mark! (:l ev))))
          (recur))))))

(defn -main []
  (println (str "[reactor] coordinator auto-export: subscribe :" port
                " (debounce " debounce-ms "ms) -> " tern-bin " heal"))
  (flush)
  (future (flusher))
  (loop []
    (try (subscribe-once)
         (catch Throwable t
           (println (str "[reactor] subscription lost (" (.getMessage t) ") — reconnecting")) (flush)))
    (Thread/sleep 1000)               ; brief backoff, then reconnect (survives a bounce)
    (recur)))

(-main)
