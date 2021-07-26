// Command serialsniff reads data from a serial bus.
package main

import (
	"log"

	"github.com/spf13/cobra"
)

func main() {
	cmd := &cobra.Command{
		Use:  "serialsniff",
		Args: cobra.ExactArgs(0),
	}

	cmd.AddCommand(sniffCommand())
	cmd.AddCommand(&cobra.Command{
		Use:  "dump FILE",
		Args: cobra.ExactArgs(1),
		RunE: dump,
	})

	if err := cmd.Execute(); err != nil {
		log.Fatalln(err)
	}
}
