import * as fs from "node:fs";
import * as path from "node:path";
import { pathToFileURL } from "node:url";

import {
	"bridge-journal-root" as bridgeJournalRoot,
} from "./bridge/generated/north/bridge/protocol.js";
import { RUN_SHARE_MAX_VIEWER_BYTES } from "./run-share-contract";
import { renderRunShareViewer } from "./run-share-viewer";
import {
	RunShareError,
	buildBridgeRunShareBundle,
	sealRunShareBundle,
} from "./run-share";
import { streamDirectory } from "./stream-writer";

const USAGE = "usage: north run-share export <bridge-uuid> --out <new-directory>";
const BUNDLE_FILE = "run.northshare";
const VIEWER_FILE = "viewer.html";
const TEXT_ENCODER = new TextEncoder();

export type RunShareCliErrorCode =
	| "invalid_arguments"
	| "invalid_output"
	| "output_exists"
	| "output_conflicts_source"
	| "output_write_failed";

export class RunShareCliError extends Error {
	readonly code: RunShareCliErrorCode;

	constructor(code: RunShareCliErrorCode, message: string, options?: ErrorOptions) {
		super(message, options);
		this.name = "RunShareCliError";
		this.code = code;
	}
}

export interface RunShareExportResult {
	readonly outputDirectory: string;
	readonly bundlePath: string;
	readonly viewerPath: string;
	readonly link: string;
}

interface ExportRequest {
	readonly executionId: string;
	readonly outputDirectory: string;
}

function parseExportRequest(args: readonly string[]): ExportRequest {
	if (args.length !== 4 || args[0] !== "export" || args[2] !== "--out"
		|| !args[1]?.trim() || !args[3]?.trim()) {
		throw new RunShareCliError("invalid_arguments", USAGE);
	}
	return { executionId: args[1], outputDirectory: args[3] };
}

function isWithin(parent: string, child: string): boolean {
	const relative = path.relative(parent, child);
	return relative === "" || (!relative.startsWith(`..${path.sep}`)
		&& relative !== ".." && !path.isAbsolute(relative));
}

function canonicalExistingOrResolved(value: string): string {
	try { return fs.realpathSync(value); }
	catch (cause) {
		if ((cause as NodeJS.ErrnoException).code !== "ENOENT") throw cause;
		return path.resolve(value);
	}
}

function outputDirectory(value: string): string {
	if (!value.trim() || value.includes("\0")) {
		throw new RunShareCliError("invalid_output", "run share output directory is invalid");
	}
	const resolved = path.resolve(value);
	if (resolved === path.parse(resolved).root) {
		throw new RunShareCliError("invalid_output", "run share output cannot be a filesystem root");
	}
	const parent = path.dirname(resolved);
	let canonicalParent: string;
	try { canonicalParent = fs.realpathSync(parent); }
	catch (cause) {
		throw new RunShareCliError(
			"invalid_output",
			"run share output parent must be an existing directory",
			{ cause },
		);
	}
	let parentStat: fs.Stats;
	try { parentStat = fs.statSync(canonicalParent); }
	catch (cause) {
		throw new RunShareCliError("invalid_output", "run share output parent is unavailable", { cause });
	}
	if (!parentStat.isDirectory()) {
		throw new RunShareCliError("invalid_output", "run share output parent must be a directory");
	}
	const canonical = path.join(canonicalParent, path.basename(resolved));
	try {
		fs.lstatSync(canonical);
		throw new RunShareCliError("output_exists", "run share output already exists; refusing to overwrite it");
	} catch (cause) {
		if (cause instanceof RunShareCliError) throw cause;
		if ((cause as NodeJS.ErrnoException).code !== "ENOENT") {
			throw new RunShareCliError("invalid_output", "run share output cannot be inspected", { cause });
		}
	}
	const sources = [bridgeJournalRoot(), streamDirectory()].map(canonicalExistingOrResolved);
	if (sources.some((source) => isWithin(source, canonical))) {
		throw new RunShareCliError(
			"output_conflicts_source",
			"run share output must be outside Bridge journals and run source data",
		);
	}
	return canonical;
}

function fsyncDirectory(directory: string): void {
	const descriptor = fs.openSync(
		directory,
		fs.constants.O_RDONLY | (fs.constants.O_DIRECTORY ?? 0),
	);
	try { fs.fsyncSync(descriptor); }
	finally { fs.closeSync(descriptor); }
}

function writePrivateFile(filePath: string, content: string | Uint8Array): void {
	const descriptor = fs.openSync(
		filePath,
		fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_EXCL
			| (fs.constants.O_NOFOLLOW ?? 0),
		0o600,
	);
	try {
		fs.writeFileSync(descriptor, content);
		fs.fsyncSync(descriptor);
	} finally {
		fs.closeSync(descriptor);
	}
	fs.chmodSync(filePath, 0o600);
}

function cleanupCreatedOutput(directory: string, files: readonly string[]): void {
	for (const file of files) {
		try { fs.unlinkSync(file); }
		catch (cause) {
			if ((cause as NodeJS.ErrnoException).code !== "ENOENT") return;
		}
	}
	try { fs.rmdirSync(directory); }
	catch { /* Retain an unexpected entry rather than delete material this command did not create. */ }
}

export async function exportBridgeRunShare(
	executionId: string,
	output: string,
): Promise<RunShareExportResult> {
	const target = outputDirectory(output);
	const bundle = await buildBridgeRunShareBundle(executionId);
	const seal = await sealRunShareBundle(bundle);
	const viewer = renderRunShareViewer(seal.sealed);
	const viewerBytes = TEXT_ENCODER.encode(viewer).byteLength;
	if (viewerBytes > RUN_SHARE_MAX_VIEWER_BYTES) {
		throw new RunShareCliError("output_write_failed", "run share viewer exceeds its byte limit");
	}
	const bundlePath = path.join(target, BUNDLE_FILE);
	const viewerPath = path.join(target, VIEWER_FILE);
	let created = false;
	try {
		fs.mkdirSync(target, { mode: 0o700 });
		created = true;
		fs.chmodSync(target, 0o700);
		writePrivateFile(bundlePath, seal.sealed);
		writePrivateFile(viewerPath, viewer);
		fsyncDirectory(target);
		fsyncDirectory(path.dirname(target));
	} catch (cause) {
		if (created) cleanupCreatedOutput(target, [bundlePath, viewerPath]);
		if ((cause as NodeJS.ErrnoException).code === "EEXIST") {
			throw new RunShareCliError("output_exists", "run share output already exists; refusing to overwrite it");
		}
		throw new RunShareCliError("output_write_failed", "run share output could not be published", { cause });
	}
	return Object.freeze({
		outputDirectory: target,
		bundlePath,
		viewerPath,
		link: `${pathToFileURL(viewerPath).href}#${seal.fragment}`,
	});
}

export async function runShareCli(args: readonly string[]): Promise<RunShareExportResult> {
	const request = parseExportRequest(args);
	return exportBridgeRunShare(request.executionId, request.outputDirectory);
}

function failureMessage(error: unknown): string {
	if (error instanceof RunShareCliError || error instanceof RunShareError) return error.message;
	return "run share export failed";
}

if (import.meta.main) {
	runShareCli(process.argv.slice(2)).then(
		(result) => process.stdout.write(`${result.link}\n`),
		(error) => {
			process.stderr.write(`north: ${failureMessage(error)}\n`);
			process.exitCode = error instanceof RunShareCliError && error.code === "invalid_arguments" ? 2 : 1;
		},
	);
}
