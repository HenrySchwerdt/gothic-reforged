// Chicken rain: every quest the hero finishes makes chickens fall from the sky.

fn on_load() {
    this.count = 12;       // chickens per rain
    this.height = 1500.0;  // cm above the hero
    this.spread = 600;     // cm around the hero
    this.rains = 0;
}

fn on_quest_changed(e) {
    if e.status != "Success" {
        return;
    }
    let h = hero_pos();
    for i in 0..this.count {
        let at = #{
            x: h.x + rand(-this.spread, this.spread).to_float(),
            y: h.y + this.height + rand(0, 800).to_float(),
            z: h.z + rand(-this.spread, this.spread).to_float(),
        };
        spawn_npc("Chicken", at, rand_float() * 6.2832);
    }
    this.rains += 1;
    notice(`${e.quest} done - it's raining chickens!`);
}
