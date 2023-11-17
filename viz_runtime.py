import seaborn as sns
import pandas as pd
import matplotlib.pyplot as plt

results = pd.read_csv("output/data/inference_times.csv")

bars = pd.DataFrame(columns=["BinSize", "Location", "Runtime"])
for bin_size in [1, 5,  10, 15, 20]:
    for location in [5]:
        subset = results[results["BinSize"] == bin_size]
        subset = subset[subset["Location"] == location]
        subset = subset[subset["Time"] < 20]

        data = pd.DataFrame(data={"BinSize": bin_size, "Location": location, "Adapted": False, "Runtime": subset["Runtime"].mean()}, index=[0])
        bars = pd.concat([bars, data], ignore_index=True)

        subset = results[results["BinSize"] == bin_size]
        subset = subset[subset["Location"] == location]
        subset = subset[subset["Time"] > 20]

        data = pd.DataFrame(data={"BinSize": bin_size, "Location": location, "Adapted": True, "Runtime": subset["Runtime"].mean()}, index=[0])
        bars = pd.concat([bars, data], ignore_index=True)

plot = sns.barplot(data=bars, x="BinSize", hue="Adapted", y="Runtime", palette=sns.color_palette())
plt.gcf().tight_layout()
sns.despine(top=True, right=True)

plot.set_xlabel("Bin size / Hz", fontsize=20)
plot.set_ylabel("Inference Time / s", fontsize=20)
plot.tick_params(labelsize=15)
# plot.legend(title="Mean FoC", fontsize=15)
plot.set_yscale("log")
plot.set_aspect(8.0 / 10.0)

plot.get_figure().savefig("output/plots/time_plot.pdf", bbox_inches='tight')
