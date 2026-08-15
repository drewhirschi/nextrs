// TypeScript's Bundler module resolution intentionally preserves extensionless
// relative specifiers. Node's ESM loader requires explicit files, so make the
// emitted package portable after tsc without changing Orval-owned source.
import { readdir, readFile, stat, writeFile } from "node:fs/promises";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const clientDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const distDir = join(clientDir, "dist");

async function isFile(path) {
  try {
    return (await stat(path)).isFile();
  } catch {
    return false;
  }
}

async function emittedSpecifier(file, specifier) {
  if (extname(specifier)) return specifier;
  const target = resolve(dirname(file), specifier);
  if (await isFile(`${target}.js`)) return `${specifier}.js`;
  if (await isFile(join(target, "index.js"))) {
    return `${specifier.replace(/\/$/, "")}/index.js`;
  }
  return specifier;
}

async function filesUnder(dir) {
  const files = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) files.push(...(await filesUnder(path)));
    else if (entry.isFile() && (path.endsWith(".js") || path.endsWith(".d.ts"))) {
      files.push(path);
    }
  }
  return files;
}

function moduleSpecifiers(file, source) {
  const sourceFile = ts.createSourceFile(
    file,
    source,
    ts.ScriptTarget.Latest,
    true,
  );
  const specifiers = [];
  const add = (literal) => {
    if (literal?.text.startsWith(".")) {
      specifiers.push({
        start: literal.getStart(sourceFile) + 1,
        end: literal.getEnd() - 1,
        value: literal.text,
      });
    }
  };
  const visit = (node) => {
    if (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) {
      add(node.moduleSpecifier);
    } else if (
      ts.isCallExpression(node) &&
      node.expression.kind === ts.SyntaxKind.ImportKeyword &&
      ts.isStringLiteral(node.arguments[0])
    ) {
      add(node.arguments[0]);
    } else if (
      ts.isImportTypeNode(node) &&
      ts.isLiteralTypeNode(node.argument) &&
      ts.isStringLiteral(node.argument.literal)
    ) {
      add(node.argument.literal);
    }
    ts.forEachChild(node, visit);
  };
  visit(sourceFile);
  return specifiers;
}

let rewritten = 0;
const anyTypes = [];
const emittedFiles = await filesUnder(distDir);
for (const file of emittedFiles) {
  const source = await readFile(file, "utf8");
  let output = "";
  let cursor = 0;
  for (const moduleSpecifier of moduleSpecifiers(file, source)) {
    const specifier = await emittedSpecifier(file, moduleSpecifier.value);
    output += source.slice(cursor, moduleSpecifier.start);
    output += specifier;
    cursor = moduleSpecifier.end;
    if (specifier !== moduleSpecifier.value) rewritten += 1;
  }
  output += source.slice(cursor);
  if (output !== source) await writeFile(file, output);

  if (file.endsWith(".d.ts")) {
    const declaration = ts.createSourceFile(
      file,
      output,
      ts.ScriptTarget.Latest,
      true,
    );
    const visit = (node) => {
      if (node.kind === ts.SyntaxKind.AnyKeyword) {
        const { line, character } = declaration.getLineAndCharacterOfPosition(
          node.getStart(declaration),
        );
        anyTypes.push(`${file}:${line + 1}:${character + 1}`);
      }
      ts.forEachChild(node, visit);
    };
    visit(declaration);
  }
}

if (anyTypes.length > 0) {
  throw new Error(
    `generated public declarations contain explicit any types:\n${anyTypes.join("\n")}`,
  );
}

// Package self-references exercise the real exports map, not a convenient
// relative file path. Keep this check in every client build.
const packageJson = JSON.parse(
  await readFile(join(clientDir, "package.json"), "utf8"),
);
await import(packageJson.name);
await import(`${packageJson.name}/react-query`);
console.log(
  `normalized ${rewritten} ESM specifiers; verified package exports and ${emittedFiles.filter((file) => file.endsWith(".d.ts")).length} declarations without any`,
);

export { emittedSpecifier, moduleSpecifiers };
