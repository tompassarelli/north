use super::*;
use clause_runtime::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

type Counts = BTreeMap<String, u64>;

pub(super) struct Recorder {
    path: PathBuf,
    session: u64,
    transaction: u64,
    occurrence_count: usize,
    handlers: Counts,
    clauses: Counts,
    selected: Counts,
    total_clauses: usize,
    host_slot: u16,
    payload_slot: u16,
}

pub(super) struct Inspection {
    observations: Vec<Value>,
    projection: Option<Term>,
}

fn failure(error: impl std::fmt::Display) -> NorthError {
    NorthError::Protocol(format!("transition recorder: {error}"))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn argument(value: &ExecutableValueV1) -> Value {
    match value {
        ExecutableValueV1::Number(bits) => json!({"number": f64::from_bits(*bits)}),
        ExecutableValueV1::Boolean(value) => json!({"boolean": value}),
        ExecutableValueV1::Text(value) => json!({"text": value.as_str()}),
        ExecutableValueV1::Referent(value) => {
            json!({"domain": value.domain(), "identity": format!("{:?}", value.identity())})
        }
        other => json!({"typed_value": format!("{other:?}")}),
    }
}

impl Recorder {
    pub(super) fn open(
        workbench: &ResidentSourceWorkbenchV1,
        restored: bool,
    ) -> NorthResult<Option<Self>> {
        let Some(path) = std::env::var_os("NORTH_TRANSITION_RECORD") else {
            return Ok(None);
        };
        let plan =
            decode_executable_physical_plan_v1(&workbench.generation().cpp1).map_err(failure)?;
        let bindings = workbench.state_bindings()?;
        let slot = |name: &[u8]| {
            bindings
                .iter()
                .find(|binding| binding.state.relation_designation == name)
                .map(|binding| binding.slot)
                .ok_or_else(|| failure("missing effect state binding"))
        };
        let recorder = Self {
            path: path.into(),
            session: NEXT_SESSION.fetch_add(1, Ordering::Relaxed),
            transaction: 0,
            occurrence_count: 0,
            handlers: Counts::new(),
            clauses: Counts::new(),
            selected: Counts::new(),
            total_clauses: plan.program.rules.len(),
            host_slot: slot(b"host-effect")?,
            payload_slot: slot(b"effect-payload")?,
        };
        recorder.write(json!({"kind": "session", "restored": restored,
            "source_sha256": format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(NORTH_SOURCE)),
            "projection_encoding": "Clause canonical term bytes, hex",
            "coverage_origin": "dispatcher occurrences; immutable Clause evaluation inspection"}))?;
        if restored {
            recorder.write(json!({"kind": "continuation", "operation": "restore",
                "projection_hex": hex(&clause_package::canonical_term_bytes(&workbench.project_current_world()?).map_err(failure)?)}))?;
        }
        Ok(Some(recorder))
    }

    fn write(&self, mut value: Value) -> NorthResult<()> {
        value["session"] = json!(self.session);
        value["transaction"] = json!(self.transaction);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(failure)?;
        serde_json::to_writer(&mut file, &value).map_err(failure)?;
        writeln!(file).map_err(failure)
    }

    pub(super) fn begin(
        &mut self,
        workbench: &ResidentSourceWorkbenchV1,
        transitions: &[(&[u8], Vec<ExecutableValueV1>)],
    ) -> NorthResult<Option<Vec<u8>>> {
        self.transaction += 1;
        self.occurrence_count = transitions.len();
        self.write(json!({"kind": "transaction", "boundary": "begin"}))?;
        for (index, (name, arguments)) in transitions.iter().enumerate() {
            *self
                .handlers
                .entry(String::from_utf8_lossy(name).into_owned())
                .or_default() += 1;
            self.write(json!({"kind": "occurrence", "ordinal": index, "handler": String::from_utf8_lossy(name),
                "arguments": arguments.iter().map(argument).collect::<Vec<_>>(), "maintenance": index + 6 >= transitions.len()}))?;
        }
        workbench
            .last_projection()
            .map(|_| workbench.checkpoint_admitted().map_err(NorthError::from))
            .transpose()
    }

    pub(super) fn inspect(
        &mut self,
        workbench: &ResidentSourceWorkbenchV1,
        occurrences: &[Vec<u8>],
        checkpoint: Option<&[u8]>,
    ) -> NorthResult<Inspection> {
        let Some(checkpoint) = checkpoint else {
            return Ok(Inspection {
                observations: vec![],
                projection: None,
            });
        };
        let request =
            decode_wasm_process_request_v1(&workbench.generation().cwr1).map_err(failure)?;
        let open = WasmSessionOpenV1 {
            package_bytes: request.package_bytes,
            application: request.application,
            physical_plan_bytes: workbench.generation().cpp1.clone(),
            authority: request.authority,
            allocation: WasmSessionAllocationV1::New,
            limits: WasmSessionLimitsV1 {
                max_commands: 4096,
                command_bytes: WASM_SESSION_COMMAND_LIMIT_V1 as u32,
                event_bytes: WASM_SESSION_EVENT_LIMIT_V1 as u32,
                trace_retention: WasmSessionTraceRetentionV1::CurrentAdmission,
            },
        };
        let mut boundary = WasmPersistentSessionBoundaryV1::new();
        let opened = boundary
            .reopen_admitted(
                &encode_wasm_session_open_v1(&open).map_err(failure)?,
                checkpoint,
            )
            .map_err(failure)?;
        let WasmSessionEventKindV1::Opened {
            package, session, ..
        } = opened.kind
        else {
            return Err(failure("inspection did not open"));
        };
        let mut sequence = opened.accepted_sequence;
        let mut observations = Vec::new();
        let mut candidate = None;
        for (index, occurrence) in occurrences.iter().enumerate() {
            let event = boundary
                .native_occurrence(
                    opened.handle,
                    sequence,
                    occurrence.clone(),
                    index + 1 == occurrences.len(),
                )
                .map_err(failure)?;
            sequence = event.accepted_sequence;
            match event.kind {
                WasmSessionEventKindV1::InputAccepted { .. } => {}
                WasmSessionEventKindV1::CandidateAccepted {
                    candidate: id,
                    base,
                    ..
                } => {
                    candidate = Some((id, base));
                }
                other => return Err(failure(format!("inspection occurrence {index}: {other:?}"))),
            }
            let entry = decode_executable_occurrence_v1(occurrence)
                .map_err(failure)?
                .entry;
            let trace = boundary
                .recorded_event(opened.handle, entry)
                .map_err(failure)?
                .ok_or_else(|| failure("missing occurrence trace"))?;
            observations.push(self.trace(index, trace)?);
        }
        let (candidate, base) = candidate.ok_or_else(|| failure("missing inspection candidate"))?;
        let issue = WasmSessionCommandV1 {
            handle: opened.handle,
            expected_sequence: sequence,
            operation: WasmSessionOperationV1::IssueAdmission(WasmSessionAdmissionScopeV1 {
                package,
                session,
                base,
                candidate,
            }),
        };
        let issued = boundary
            .command(&encode_wasm_session_command_v1(&issue).map_err(failure)?)
            .map_err(failure)?;
        let WasmSessionEventKindV1::AdmissionAuthorizationIssued { occurrence, .. } = issued.kind
        else {
            return Err(failure("inspection authorization failed"));
        };
        let admit = WasmSessionCommandV1 {
            handle: opened.handle,
            expected_sequence: issued.accepted_sequence,
            operation: WasmSessionOperationV1::Admit(WasmSessionAdmissionV1 {
                package,
                session,
                base,
                candidate,
                authorization: occurrence,
            }),
        };
        let admitted = boundary
            .command(&encode_wasm_session_command_v1(&admit).map_err(failure)?)
            .map_err(failure)?;
        let WasmSessionEventKindV1::AdmissionAccepted {
            projection: Some(projection),
            ..
        } = admitted.kind
        else {
            return Err(failure("inspection admission failed"));
        };
        Ok(Inspection {
            observations,
            projection: Some(projection.term),
        })
    }

    fn trace(&mut self, index: usize, trace: &ExecutableRecordedEventV1) -> NorthResult<Value> {
        if trace.trace.truncated {
            return Err(failure("Clause coverage trace truncated"));
        }
        let mut effects = Vec::new();
        let mut clauses = Vec::new();
        for rule in &trace.trace.rules {
            let key = rule.rule.to_string();
            *self.clauses.entry(key.clone()).or_default() += 1;
            if rule.selected {
                *self.selected.entry(key).or_default() += 1;
            }
            clauses.push(json!({"rule": rule.rule, "selected": rule.selected}));
            for effect in &rule.effects {
                effects.push(json!({"rule": rule.rule, "slot": effect.slot, "additive": effect.additive,
                    "subject": effect.subject.as_ref().map(|subject| argument(&ExecutableValueV1::Referent(subject.clone()))),
                    "value": effect.evaluated.as_ref().map(|evaluation| argument(&evaluation.value))}));
            }
        }
        let host_effects = if trace
            .trace
            .rules
            .iter()
            .flat_map(|rule| &rule.effects)
            .any(|effect| effect.slot == self.host_slot)
        {
            let text = |slot: u16| -> Option<&str> {
                let value = trace.after.get(usize::from(slot))?.value()?;
                match value {
                    ExecutableValueV1::RelationTable(table) => {
                        table.rows().values().flatten().next()?.as_text()
                    }
                    other => other.as_text(),
                }
            };
            match text(self.host_slot).filter(|action| !action.is_empty()) {
                Some(action) => vec![json!({"action": action, "payload": text(self.payload_slot)})],
                None => vec![],
            }
        } else {
            vec![]
        };
        Ok(
            json!({"kind": "evaluation", "ordinal": index, "rule_applied": trace.step.rule_applied,
            "clauses": clauses, "ordered_state_effects": effects, "ordered_host_effects": host_effects}),
        )
    }

    pub(super) fn finish(
        &mut self,
        workbench: &ResidentSourceWorkbenchV1,
        transitions: &[(&[u8], Vec<ExecutableValueV1>)],
        mut inspection: Inspection,
        admission: &clause_workbench::ResidentSourceAdmissionV1,
    ) -> NorthResult<()> {
        if let Some(projected) = &inspection.projection {
            if projected != &admission.projection.term {
                return Err(failure(
                    "inspection projection differs from actual dispatcher admission",
                ));
            }
        } else {
            for (index, (designation, _)) in transitions.iter().enumerate() {
                let event = workbench
                    .recorded_event(designation)?
                    .ok_or_else(|| failure("missing initialization trace"))?;
                inspection.observations.push(self.trace(index, event)?);
            }
        }
        let ordered_effects = inspection
            .observations
            .iter()
            .flat_map(|observation| {
                observation["ordered_host_effects"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .cloned()
            })
            .collect::<Vec<_>>();
        for observation in inspection.observations {
            self.write(observation)?;
        }
        for (index, _) in transitions.iter().enumerate() {
            self.write(json!({"kind": "admission", "ordinal": index, "result": "admitted"}))?;
        }
        let mut decoded = BTreeMap::new();
        let mut chat = None;
        let projected = decode_projection(&admission.projection.term, &mut decoded, &mut chat)?;
        let effects = if projected.host_effect.is_empty() {
            vec![]
        } else {
            vec![json!({"action": projected.host_effect, "payload": projected.effect_payload})]
        };
        self.write(
            json!({"kind": "effects", "ordered": ordered_effects, "mailbox_at_boundary": effects}),
        )?;
        self.write(json!({"kind": "projection", "projection_hex": hex(&admission.projection.exact_term_bytes()),
            "menu": {"kind": projected.menu.kind, "title": projected.menu.title, "query": projected.menu.query,
                "selection": projected.menu.selection, "rows": projected.menu.rows.iter().map(|row| json!({"key": row.key, "label": row.label})).collect::<Vec<_>>()}}))?;
        self.write(json!({"kind": "transaction", "boundary": "commit",
            "inspection_projection_matches": inspection.projection.is_some(),
            "revision": format!("{:?}", admission.successor)}))?;
        self.write(json!({"kind": "continuation", "operation": "checkpoint", "revision": format!("{:?}", admission.successor)}))?;
        self.coverage()
    }

    pub(super) fn reject(&self, error: &dyn std::fmt::Display) -> NorthResult<()> {
        for ordinal in 0..self.occurrence_count {
            self.write(
                json!({"kind": "admission", "ordinal": ordinal, "result": "rejected",
                "rejection": error.to_string()}),
            )?;
        }
        self.write(json!({"kind": "transaction", "boundary": "reject"}))
    }

    fn coverage(&self) -> NorthResult<()> {
        let path = self.path.with_extension("coverage.json");
        let mut sessions: BTreeMap<String, Value> = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(failure)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(failure(error)),
        };
        sessions.insert(self.session.to_string(), json!({"transactions": self.transaction,
            "handler_dispatches": self.handlers, "clause_evaluations": self.clauses,
            "clause_selections": self.selected, "total_compiled_clauses": self.total_clauses,
            "handler_origin": "actual North transition_sequence dispatcher, including text-selected and maintenance occurrences",
            "clause_origin": "immutable Clause evaluation inspection from identical admitted checkpoint; exact resulting projection checked against actual dispatch; initialization uses actual retained trace",
            "scope": "this recorded sequence only; compiled clause ordinals are specific to the pinned Clause compiler and source"}));
        std::fs::write(path, serde_json::to_vec_pretty(&sessions).map_err(failure)?)
            .map_err(failure)
    }
}
