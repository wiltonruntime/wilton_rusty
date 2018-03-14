
define([
    "wilton/dyload",
    "wilton/wiltoncall"
], function(dyload, wiltoncall) {
    "use strict";

    return {
        main: function() {
            dyload({
                name: "wilton_rust",
                directory: "target/debug"
            });
            var res = wiltoncall("foo", {
                bar: 41,
                baz: 42
            });
            print("[" + res + "]");
            var res = wiltoncall("bar", {
                bar: 43,
                baz: 44
            });
            print("[" + res + "]");
        }
    };
});
