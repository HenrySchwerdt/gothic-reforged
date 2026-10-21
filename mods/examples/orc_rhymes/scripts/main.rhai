// All orcs speak in rhymes: every orc subtitle gets a tail that rhymes with its last word.

fn on_load() {
    this.rhymes = #{
        an: ["that is the orcish plan", "run while you can, little man"],
        ight: ["Kan smells a fight", "by day or by night"],
        ay: ["the orcs are here to stay", "now go away"],
        ore: ["the orcs want more", "and then you hit the floor"],
        ead: ["or you end up dead", "Kan has spoken, enough said"],
        ing: ["hear the war drums ring", "Rhobar is no king"],
        ere: ["the orcs have no fear", "the orcs rule here"],
        old: ["give the orcs your gold", "or you will not grow old"],
    };
    this.fallback = ["that is the orcish rhyme, every time", "orc words, orc rhyme"];
    this.lines = 0;
}

// The last word of a text, lower case, letters only.
fn last_word(text) {
    let words = text.split(" ");
    let word = "";
    for ch in words[words.len() - 1].to_lower() {
        if ch >= 'a' && ch <= 'z' {
            word += ch;
        }
    }
    word
}

fn rewrite_line(line) {
    if line.species != "Orc" || line.text == "" {
        return;
    }
    let word = last_word(line.text);
    let tails = this.fallback;
    for ending in this.rhymes.keys() {
        if word.ends_with(ending) {
            tails = this.rhymes[ending];
            break;
        }
    }
    this.lines += 1;
    let text = line.text;
    while text.len() > 0 && "!.?,".contains(text[text.len() - 1]) {
        text.pop();
    }
    `${text} - ${tails[rand(0, tails.len() - 1)]}!`
}
