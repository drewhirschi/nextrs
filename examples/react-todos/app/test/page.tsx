import { getApiTodosById } from "@react-todos/client";
import { useGetApiTodosById } from "@react-todos/client/react-query";
import { useEffect, useState } from "react";

export default function TestPage() {
  const [title, setTitle] = useState("");
  useEffect(() => {
    getApiTodosById(1, { neighbors: true }).then((data) => {
      console.log("getApiTodosById", data);
      setTitle(data.data.title);
    });
  }, []);

  return <div>{title}</div>;
}
