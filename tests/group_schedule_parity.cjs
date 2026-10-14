// Uses the checked-in ST source directly; does not copy its scheduling algorithms.
const fs = require('node:fs');
const vm = require('node:vm');
const cp = require('node:child_process');
const path = require('node:path');
const root = path.resolve(__dirname, '..');
const groupSource = fs.readFileSync(path.join(root, 'refrence/SillyTavern/public/scripts/group-chats.js'), 'utf8');
const utilSource = fs.readFileSync(path.join(root, 'refrence/SillyTavern/public/scripts/utils.js'), 'utf8');
function original(source, name) {
  const start = source.indexOf(`function ${name}(`);
  if (start < 0) throw new Error(`missing ST function ${name}`);
  const end = source.indexOf('\n}', start) + 2;
  return source.slice(start, end);
}
const source = ['activateNaturalOrder','activatePooledOrder','activateListOrder'].map(n => original(groupSource, n)).concat(['extractAllWords','shuffle'].map(n => original(utilSource, n))).join('\n');
const cases = [], expected = [];
for (let i = 0; i < 12000; i++) {
  let seed = (0x12345678 + i * 7919) >>> 0;
  let randomState = seed;
  const random = () => { randomState ^= randomState << 13; randomState ^= randomState >>> 17; randomState ^= randomState << 5; return (randomState >>> 0) / 4294967296; };
  const n = i % 9 + 1;
  const names = ['Alice Smith','Bob','Alice Smith','小明','ALICE','Jean-Luc','A_B','Éva','7'];
  const avatars = Array.from({length:n}, (_,j) => `${j}.png`);
  const characters = avatars.filter((_,j) => !(i%11===0 && j===n-1)).map((avatar,j) => ({avatar, name:names[j], talkativeness:[0,.1,.5,1,-1,2,null][(i+j)%7]}));
  const disabled = avatars.filter((_,j) => (i+j)%5===0 || i%17===0);
  const members = avatars.filter(a => !disabled.includes(a));
  const mode = Math.floor(i/9)%4;
  const last = i%4 === 0 ? {is_user:true,mes:'user'} : {is_user:false,name:names[i%n],original_avatar:avatars[i%n]};
  const user = i%3===0;
  const spoken = user ? [] : avatars.filter((_,j) => (i+j)%3!==0);
  const input = ['Bob! Alice Smith','Jean-Luc A_B','小明 Éva','ALICE\nBob','unrelated',''][i%6];
  const context = {characters, chat:[{is_user:true},...spoken.map(a => ({original_avatar:a,is_user:false}))],
    Math:Object.assign(Object.create(Math), {random}), talkativeness_default:.5, onlyUnique:(v,i,a)=>a.indexOf(v)===i, system_message_types:{NARRATOR:'narrator'}};
  vm.createContext(context); vm.runInContext(source,context);
  context.members=members; context.last=last; context.input=input; context.user=user; context.self=i%2===0;
  const expression = mode===0 ? 'activateNaturalOrder(members,input,last,self,user)' : mode===1 ? 'activateListOrder(members)' : mode===3 ? 'activatePooledOrder(members,last,user)' : 'user ? [] : shuffle([...members]).slice(0,1).map(x=>characters.findIndex(y=>y.avatar===x)).filter(x=>x!==-1)';
  expected.push(Array.from(vm.runInContext(expression,context), j=>characters[j].avatar));
  // ST treats null talkativeness as Number(null) = 0.
  cases.push({seed,group:{members:avatars,disabled_members:disabled,activation_strategy:mode,allow_self_responses:i%2===0},characters:characters.map(c=>({...c,talkativeness:c.talkativeness??0})),last,is_user:user,input,unspoken:avatars.filter(a=>!spoken.includes(a))});
}
cp.execFileSync('cargo',['build','-p','nast-engine','--example','group_schedule','--locked'],{cwd:root,stdio:'inherit'});
const executable = path.join(root, 'target/debug/examples/group_schedule' + (process.platform==='win32'?'.exe':''));
const run = cp.spawnSync(executable, [], {input:cases.map(c=>JSON.stringify(c)).join('\n'),encoding:'utf8',maxBuffer:32*1024*1024});
if(run.status !== 0) throw new Error(run.stderr);
const actual=run.stdout.trim().split(/\r?\n/).map(JSON.parse);
const failures=[];
cases.forEach((value,i)=>{if(JSON.stringify(actual[i])!==JSON.stringify(expected[i])) failures.push({index:i,input:value,expected:expected[i],actual:actual[i]});});
fs.mkdirSync(path.join(root,'.cache/groupchat'),{recursive:true});
fs.writeFileSync(path.join(root,'.cache/groupchat/scheduling.json'),JSON.stringify({reference:'8172dcd0ee672d3cd9a5e5f7af134f91a45cd2b8',count:cases.length,failures},null,2));
console.log(`${cases.length} ST scheduler cases, ${failures.length} failures`);
if(failures.length) { console.error(failures.slice(0,3)); process.exitCode=1; }
