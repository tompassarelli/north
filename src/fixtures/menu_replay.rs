use crate::menu::MenuEvent;
pub fn corpus() -> Vec<Vec<MenuEvent<'static>>> {
    use MenuEvent::*;
    let mut tape = vec![
        vec![Move(1)], vec![Accept], vec![ClearRows],
        vec![OpenSettings { kind:"document", title:"Guide" }],
        vec![OpenHistory { kind:"history" }],
        vec![OfferRow { key:"", label:"Current", description:"Straße", annotation:"ignored" }],
        vec![OfferRow { key:"b", label:"Beta", description:"DETAIL", annotation:"on" }],
        vec![OfferRow { key:"c", label:"Gamma", description:"Third", annotation:"off" }],
        vec![Move(-10)],vec![Move(20)],vec![Move(1)],
        vec![Query("detail")],vec![Accept],vec![Accept],
        vec![ClearEffect],vec![OpenHistory { kind:"archived-history" }],vec![Accept],
        vec![Close],vec![ClearEffect],vec![Query("no matches")],vec![Move(1)],
        vec![ClearRows,OpenSettings {kind:"usage",title:"Usage"}],
        vec![Query(""), Move(-2)],
    ];
    for kind in ["work","models","efforts","config","other"] {
        tape.push(vec![ClearRows,OpenSettings {kind,title:"Settings"},OfferRow {key:"first",label:"Alpha",description:"One",annotation:"ready"},OfferRow {key:"second",label:"Beta",description:"Two",annotation:"held"},Query("")]);
        tape.push(vec![Move(1),Accept]); tape.push(vec![Accept]);
        tape.push(vec![ClearEffect]); tape.push(vec![Accept]);
        tape.push(vec![ClearEffect]);
    }
    tape.extend([
        vec![ClearRows,OpenSettings {kind:"work",title:"Work"},OfferRow {key:"same",label:"One",description:"",annotation:"ready"},Query("")],
        vec![Accept,ClearEffect,Accept],vec![ClearEffect],
        vec![OpenSettings {kind:"work",title:"Reset order"},OfferRow {key:"different",label:"Two",description:"",annotation:"ready"}],
        vec![Accept],vec![Query("Two")],vec![Accept],vec![ClearEffect],
        vec![ClearRows,OpenSettings {kind:"work",title:"Work"},OfferRow {key:"a",label:"One",description:"",annotation:"ready"},OfferRow {key:"b",label:"Two",description:"",annotation:"ready"},Query("")],
        vec![Accept,ClearEffect,Move(1),Accept],
        vec![Query(""),Move(-1),Close],vec![],
    ]);
    tape
}
