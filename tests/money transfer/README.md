workflow notes


draft and human in the loop of specs
additions to draft and potentially also changes

??create a dependency graph and use hashes for the individual files. Keep in a hidden structure so that recreations are only for the files affected directly by the changes or those that need recompilation or might need changes because signatures have changed??

would it be enough to keep a hash of each individual file and the run the draft-spec-code workflow for all changed files and the recompile and fix errors?