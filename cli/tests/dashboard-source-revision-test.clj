#!/usr/bin/env bb
(require '[clojure.java.io :as io]
         '[clojure.string :as str])

(when-not (= "1" (System/getenv "NORTH_DASHBOARD_LIB"))
  (binding [*out* *err*]
    (println "dashboard-source-revision-test requires NORTH_DASHBOARD_LIB=1"))
  (System/exit 2))

(def test-script (or (System/getProperty "babashka.file") *file*))
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

(defn env-reader [values]
  (fn [name] (get values name)))

(defn identity [name environment]
  (with-redefs [run (fn [& _] {:ok true :out "tree-head\n" :err ""})]
    (source-revision name "/unused" (env-reader environment))))

(let [checkout-revision "checkout-clean"
      packaged-revision "package-clean"
      dirty-revision "checkout-byte-preserved-dirty"
      checkout (identity "north" {"NORTH_PACKAGE_MODE" "checkout"
                                  "NORTH_PACKAGE_REV" checkout-revision})
      packaged (identity "north" {"NORTH_PACKAGE_MODE" "nix-store"
                                  "NORTH_PACKAGE_REV" packaged-revision})
      dirty (identity "north" {"NORTH_PACKAGE_MODE" "checkout"
                               "NORTH_PACKAGE_REV" dirty-revision})]
  (check "checkout mode selects checkout provenance"
         (= {:revision checkout-revision
             :origin "checkout rev"
             :package-mode "checkout"}
            checkout))
  (check "nix-store mode selects package provenance"
         (= {:revision packaged-revision
             :origin "package rev"
             :package-mode "nix-store"}
            packaged))
  (check "dirty checkout revision remains byte-exact"
         (= dirty-revision (:revision dirty)))
  (check "dirty checkout remains checkout provenance"
         (= "checkout rev" (:origin dirty)))
  (check "Beagle Store identity comes from its current checkout"
         (= {:revision "tree-head" :origin "tree HEAD"}
            (identity "store" {})))
  (check "Beagle package identity remains unchanged"
         (= {:revision "beagle-package" :origin "package rev"}
            (identity "beagle" {"BEAGLE_PACKAGE_REV" "beagle-package"})))
  (check "checkout suppresses the nix-store application note"
         (nil? (runtime-source-note "checkout" "checkout rev")))
  (check "package mode keeps the embedded-revision note"
         (= "         (installed via nix store; embedded package revision shown above)"
            (runtime-source-note "nix-store" "package rev")))
  (check "tree identity keeps the checkout-context note"
         (= "         (installed via nix store; tree HEAD is checkout context, not the store closure identity)"
            (runtime-source-note nil "tree HEAD"))))

(let [failed (remove second @checks)]
  (println (str "dashboard source revision: "
                (- (count @checks) (count failed))
                " / " (count @checks) " PASS"))
  (System/exit (if (empty? failed) 0 1)))
