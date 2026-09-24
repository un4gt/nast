// SillyTavern 1.18.0 regexFromString / runRegexScript compatibility.
// Only RegExp operations are evaluated; the supplied pattern is data, never source.
(() => {
    const p = JSON.parse(payload);
    const parsed = p.find.match(/(\/?)(.+)\1([a-z]*)/i);
    if (!parsed) throw new Error('Invalid regular expression');
    const regex = parsed[3] && !/^(?!.*?(.).*?\1)[gmixXsuUAJ]+$/.test(parsed[3])
        ? new RegExp(p.find) : new RegExp(parsed[2], parsed[3]);
    if (p.test) return JSON.stringify(regex.test(p.raw));
    const pieces = [];
    let previous = 0;
    p.raw.replace(regex, function (match, ...rest) {
        const args = [match, ...rest];
        const named = typeof args[args.length - 1] === 'object';
        const offset = args[args.length - (named ? 3 : 2)];
        const replacement = p.replace.replace(/{{match}}/gi, '$0')
            .replace(/\$(\d+)|\$<([^>]+)>/g, (_, number, name) => {
                let value = number ? args[Number(number)] : (named ? args[args.length - 1][name] : '');
                if (!value) return '';
                for (const trim of p.trim) value = String(value).split(trim).join('');
                return value;
            });
        pieces.push([false, p.raw.slice(previous, offset)], [true, replacement]);
        previous = offset + match.length;
        return match;
    });
    pieces.push([false, p.raw.slice(previous)]);
    return JSON.stringify(pieces);
})()
