;; orchestration-selection.clj — shared canonical selection-rule enumeration +
;; digest for the Gaffer -> North Orchestration migration (thread 019f8f5c).
;;
;; ONE canonicalization authority, loaded by both the importer (writes the
;; policy_sha256 fact) and the projector (recomputes it at admission for the
;; §3.2 digest pin). Keeping the enumeration and the digest in a single place
;; is what makes the pin meaningful: stored, projected, and validator digests
;; are byte-comparable only because they run identical code. A divergence here
;; would make the pin refuse admission (fail closed), never silently pass.
(ns north.orchestration-selection
  (:require [clojure.java.io :as io]
            [cheshire.core :as json]
            [babashka.process :as p]))

;; Rule field order is load-bearing: the SHA-256 is taken over cheshire's
;; JSON of these maps, so the key order here IS the wire order. It matches the
;; validator-enumeration object literal below and the graph reconstruction.
(defn rule-map [signal signal-value rule-code min-tier min-reasoning]
  (array-map "signal" signal "signal_value" signal-value
             "rule_code" rule-code "min_tier" min-tier "min_reasoning" min-reasoning))

(defn rules-digest
  "SHA-256 hex over the canonical JSON of the rules, sorted by rule_code."
  [rules]
  (let [canon (json/generate-string (sort-by #(get % "rule_code") rules))
        bytes (.digest (java.security.MessageDigest/getInstance "SHA-256")
                       (.getBytes canon java.nio.charset.StandardCharsets/UTF_8))]
    (apply str (map #(format "%02x" %) bytes))))

;; Enumerate the canonical route floors FROM the validator (never a hand
;; mirror): probe each single signal value through deriveSelectionAssessment so
;; the recorded floor is exactly what the canonical policy computes. Baseline
;; holds every other signal at its no-route floor.
(defn- selection-enum-js [mjs-url]
  (str "
import { deriveSelectionAssessment } from '" mjs-url "';
const SIGNAL_VALUES = {
  decisionOwnership: ['none','bounded','cross-boundary','system-shaping','open-solution-class'],
  seamScope: ['none','established','consequential','system-wide'],
  errorExposure: ['contained-reversible','material-recoverable','high-or-hard-to-reverse'],
  oracleStrength: ['not-applicable','objective-local','objective-end-to-end','partial','judgment-only'],
  foundationalImpact: ['none','implementation-only','invariant-decision-owned'],
  dependencyShape: ['atomic-cohesive','deterministic-workflow','parallel-breadth','dynamic-decomposition','tightly-coupled-sequential'],
  reasoningShape: ['deterministic','bounded-branching','multi-hypothesis','system-synthesis','exceptional'],
};
const kebab = { decisionOwnership:'decision-ownership', seamScope:'seam-scope', errorExposure:'error-exposure', oracleStrength:'oracle-strength', foundationalImpact:'foundational-impact', dependencyShape:'dependency-shape', reasoningShape:'reasoning-shape' };
const baseline = { decisionOwnership:'none', seamScope:'none', errorExposure:'contained-reversible', oracleStrength:'not-applicable', foundationalImpact:'none', dependencyShape:'atomic-cohesive', reasoningShape:'deterministic' };
const rules = [];
for (const [sig, values] of Object.entries(SIGNAL_VALUES)) {
  for (const v of values) {
    const code = `${kebab[sig]}:${v}`;
    const signals = { ...baseline, [sig]: v };
    const d = deriveSelectionAssessment(signals);
    if (!d.ruleCodes.includes(code)) continue; // this value imposes no route floor
    rules.push({ signal: sig, signal_value: v, rule_code: code, min_tier: d.minimumTier, min_reasoning: d.minimumReasoning });
  }
}
process.stdout.write(JSON.stringify(rules));
"))

;; The Gaffer contract root may be read-only (nix store), so the probe module
;; lives in a writable temp dir and imports the canonical validator by URL.
(defn enumerate-selection-rules [root]
  (let [mjs (io/file root "scripts" "selection-assessment.mjs")
        url (str (.toURI (.getCanonicalFile mjs)))
        js (java.io.File/createTempFile "north-selection-enum" ".mjs")]
    (try
      (spit js (selection-enum-js url))
      (let [{:keys [exit out err]} (p/sh "node" (.getCanonicalPath js))]
        (when-not (zero? exit)
          (throw (ex-info (str "selection enum failed: " err) {})))
        (json/parse-string out))
      (finally (.delete js)))))
