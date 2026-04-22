const fs = require('fs');
const path = require('path');

const filePath = path.join(__dirname, 'src', 'main.ts');
let content = fs.readFileSync(filePath, 'utf8');
const lines = content.split(/\r?\n/);
lines.splice(54, 0, '  use_global_credentials?: boolean;');
fs.writeFileSync(filePath, lines.join('\n'));
console.log('Update complete log_directory to add use_global credentials in saved session at line 54.');
