package test

import rego.v1

default some_match := false

some_match if {
	input.method == "GET"
}


