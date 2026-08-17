;; validate_test.clj — north's WORK-semantics integrity rules, lifted out of
;; the fram kernel into north.validate: a depends_on edge to a withdrawn
;; (abandoned) thread, and person-ref integrity (lead/driver/proposed_by must
;; point at a node carrying a `display_name`). Plus: north.validate composes
;; these ON TOP of the engine's generic rules (cycles/dangling), so violations-i
;; surfaces both. North also owns the stronger thread-only target shape for
;; part_of/depends_on/relates_to; generic refs may target any fact-bearing
;; entity. (The generic half is covered in fram/tests/kernel_violations_test.clj.)
;;   bb -cp out:$STORE/out tests/validate_test.clj      (run from the repo root)
(require '[store.types :as t] '[north.projections :as proj]
         '[north.validate :as val])

(defn idx-of [facts] (proj/index-triples facts))
(defn has? [v sub] (some #(clojure.string/includes? % sub) v))
(defn wv [facts te] (val/work-violations-i (idx-of facts) te))
(defn fv [facts te] (val/violations-i (idx-of facts) te))

;; @p is a person (display_name). @w1 lead @p resolves cleanly.
(def ok-facts
  [(t/triple "@p" "display_name" "Tom")
   (t/triple "@w1" "title" "W1")
   (t/triple "@w1" "lead" "@p")])

;; @w2 driver @ghost — @ghost has no display_name => dangling person ref.
(def ghost-facts
  [(t/triple "@p" "display_name" "Tom")
   (t/triple "@w2" "title" "W2")
   (t/triple "@w2" "driver" "@ghost")])

;; @w3 proposed_by @p (ok) + @ghost (dangling) — only @ghost flags.
(def proposed-facts
  [(t/triple "@p" "display_name" "Tom")
   (t/triple "@w3" "title" "W3")
   (t/triple "@w3" "proposed_by" "@p")
   (t/triple "@w3" "proposed_by" "@ghost")])

;; @w4 (open) depends_on @dead; @dead is abandoned => points-at-abandoned.
(def abandoned-facts
  [(t/triple "@w4" "title" "W4")
   (t/triple "@dead" "title" "DEAD")
   (t/triple "@dead" "abandoned" "2026-01-01")
   (t/triple "@w4" "depends_on" "@dead")])

;; a RESOLVED thread's stale dep is NOT flagged (term? short-circuits).
(def abandoned-terminal
  [(t/triple "@w4" "title" "W4")
   (t/triple "@w4" "outcome" "shipped")
   (t/triple "@dead" "title" "DEAD")
   (t/triple "@dead" "abandoned" "2026-01-01")
   (t/triple "@w4" "depends_on" "@dead")])

;; composition: full violations-i = engine-generic ++ north-work.
(def mixed-facts
  [(t/triple "@w5" "title" "W5")
   (t/triple "@w5" "driver" "@ghost")
   (t/triple "@w5" "depends_on" "@missing")])

;; Fact-bearing titleless entities exist generically, but North's three thread
;; relationship predicates must still reject them.
(def non-thread-target
  [(t/triple "@w6" "title" "W6")
   (t/triple "@entity" "kind" "integration_link")
   (t/triple "@w6" "part_of" "@entity")
   (t/triple "@w6" "depends_on" "@entity")
   (t/triple "@w6" "relates_to" "@entity")])

;; A different declared ref predicate may intentionally target that same
;; titleless entity: this is the Linear integration-link shape.
(def integration-link
  [(t/triple "@linear_link" "value_kind" "ref")
   (t/triple "@w7" "title" "W7")
   (t/triple "@w7" "linear_link" "@link:linear:fixture")
   (t/triple "@link:linear:fixture" "kind" "integration_link")])

(def missing-integration-link
  [(t/triple "@linear_link" "value_kind" "ref")
   (t/triple "@w7" "title" "W7")
   (t/triple "@w7" "linear_link" "@link:linear:missing")])

;; Once any value_kind metadata exists, Beagle Store intentionally uses only the declared
;; ref predicates. North's thread-only rules must remain complete even during a
;; partial schema migration where depends_on has not been declared yet.
(def partial-schema-thread-ref
  [(t/triple "@linear_link" "value_kind" "ref")
   (t/triple "@w8" "title" "W8")
   (t/triple "@w8" "depends_on" "@missing-thread")])

(def checks
  [["lead -> resolvable actor => no violation"
    (not (has? (wv ok-facts "@w1") "references unknown person"))]
   ;; Actor refs are deliberately NOT integrity-checked: an absent actor is
   ;; ambiguous (reaped / pruned / never-registered / wrong namespace) and
   ;; disambiguating it is a retention-policy question, not a structural one.
   ;; See the note in north.validate.
   ["driver -> unresolvable actor => NO violation (not a structural defect)"
    (empty? (wv ghost-facts "@w2"))]
   ["proposed_by -> unregistered actor => NO violation"
    (empty? (wv proposed-facts "@w3"))]
   ["depends_on -> abandoned flagged for an OPEN thread"
    (has? (wv abandoned-facts "@w4") "depends_on points at abandoned @dead")]
   ["depends_on -> abandoned NOT flagged for a RESOLVED thread"
    (not (has? (wv abandoned-terminal "@w4") "points at abandoned"))]
   ["full validate composes generic ++ work"
    (let [vs (val/violations-i (idx-of mixed-facts) "@w5")]
      (and (has? vs "depends_on references missing entity @missing")
           ;; the generic engine rule still fires; the actor ref does not
           (not (has? vs "driver"))))]
   ["North thread refs reject a fact-bearing non-thread target"
    (let [vs (wv non-thread-target "@w6")]
      (and (has? vs "part_of references non-thread entity @entity")
           (has? vs "depends_on references non-thread entity @entity")
           (has? vs "relates_to references non-thread entity @entity")))]
   ["North permits a generic Linear ref to a titleless integration-link entity"
    (empty? (fv integration-link "@w7"))]
   ["North still rejects a generic ref whose target subject is absent"
    (has? (fv missing-integration-link "@w7")
          "linear_link references missing entity @link:linear:missing")]
   ["partial generic schema cannot disable North's thread-target integrity"
    (has? (fv partial-schema-thread-ref "@w8")
          "depends_on references missing thread @missing-thread")]])

(let [fails (remove second checks)]
  (doseq [[nm ok] checks] (println (if ok "  [PASS] " "  [FAIL] ") nm))
  (if (empty? fails)
    (println "\nnorth.validate:" (count checks) "/" (count checks) "PASS")
    (do (println "\nnorth.validate:" (count fails) "FAILED") (System/exit 1))))
