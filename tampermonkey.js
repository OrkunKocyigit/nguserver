// ==UserScript==
// @name         NGU Idle Send To NGU Server
// @namespace    http://tampermonkey.net/
// @version      2024-06-16
// @description  Sends current optimizer result to server for file update
// @author       Orkun Kocyigit
// @match        https://gmiclotte.github.io/gear-optimizer/
// @icon         https://www.google.com/s2/favicons?sz=64&domain=github.io
// @grant        none
// ==/UserScript==

(function() {
    'use strict';

    let buttons = document.getElementsByClassName("button-section")[0];
    let button = document.createElement("button");
    button.innerText = "Sync to NGU Server";
    button.style.width = "50%";
    button.addEventListener("click", function(e) {
        let appStates = [];
        let reactRoot = document.getElementById('app');
        let base;

        try {
            base = reactRoot._reactRootContainer._internalRoot.current;
        } catch (e) {
            console.log('Could not get internal root information from reactRoot element');
        }

        let state;
        while (base) {
            try {
                state = base.pendingProps.store.getState();
                appStates.push(state);
            } catch (e) {
            }
            base = base.child;
        }
        let $r = appStates[0].optimizer;
        let x = JSON.stringify($r.savedequip.map((x) => {if (x.name !== undefined){return {[x.name]:x.accessory.concat(x.head).concat(x.boots).concat(x.armor).concat(x.pants).concat(x.weapon).filter(x => x < 10000)}} }).filter(x => x !== undefined));
        fetch("http://localhost:3000", { method: "POST", body: x, headers: {
                "Content-Type": "application/json",
            }}).finally(() => console.log("Done"));
    });
    buttons.appendChild(button);
})();