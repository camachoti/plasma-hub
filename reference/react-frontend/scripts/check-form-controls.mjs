import { readdir, readFile } from 'node:fs/promises';
import path from 'node:path';
import ts from 'typescript';

const sourceRoot = path.resolve('src');
const componentPath = path.join(sourceRoot, 'design-system', 'TextField.tsx');
const nonTextInputTypes = new Set(['checkbox', 'radio', 'file', 'hidden', 'range', 'color', 'button', 'submit', 'reset', 'image']);
const violations = [];

async function collectTsx(directory) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const fullPath = path.join(directory, entry.name);
    if (entry.isDirectory()) files.push(...await collectTsx(fullPath));
    else if (entry.isFile() && entry.name.endsWith('.tsx')) files.push(fullPath);
  }
  return files;
}

for (const filePath of await collectTsx(sourceRoot)) {
  const source = await readFile(filePath, 'utf8');
  const sourceFile = ts.createSourceFile(filePath, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const isDesignSystemTextField = filePath === componentPath;

  function visit(node) {
    if (ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) {
      const tagName = node.tagName.getText(sourceFile);
      if (!isDesignSystemTextField && (tagName === 'input' || tagName === 'textarea' || tagName === 'select')) {
        const typeAttribute = node.attributes.properties.find(attribute =>
          ts.isJsxAttribute(attribute) && attribute.name.getText(sourceFile) === 'type');
        const type = typeAttribute && ts.isJsxAttribute(typeAttribute) && typeAttribute.initializer &&
          ts.isStringLiteral(typeAttribute.initializer) ? typeAttribute.initializer.text.toLowerCase() : '';

        if (tagName !== 'input' || !nonTextInputTypes.has(type)) {
          const { line } = sourceFile.getLineAndCharacterOfPosition(node.getStart(sourceFile));
          violations.push(`${path.relative(process.cwd(), filePath)}:${line + 1} uses <${tagName}> directly; use TextField, TextArea, or Select from the design system.`);
        }
      }
    }
    ts.forEachChild(node, visit);
  }

  visit(sourceFile);
}

if (violations.length) {
  console.error('Form controls must use the shared design system:');
  for (const violation of violations) console.error(`  ${violation}`);
  process.exitCode = 1;
} else {
  console.log('Form control check passed: text fields use the shared design system.');
}
